//! `cargo run -p xtask -- web-build` (Fase 3, T4): compila los crates WASM con el
//! perfil `release-wasm`, pasa `wasm-bindgen` y `wasm-opt -Oz`, nombra cada crate
//! por el hash de su contenido (`<out>/<hash>/…`) y escribe `<out>/manifest.json`.
//! `<out>` es `<directorio de compilación>/web`, resuelto como cargo (preflight R6),
//! y se instala con `comandos install --stage --web <out>` (origen explícito). Solo
//! se reemplaza entero y al final: un build fallido no crea ni toca `<out>`.
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
            .map(|v| !wasm_opt_version_ok(v))
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

/// `wasm-opt version 116 (version_116)`: la palabra que sigue a `version` es
/// exactamente `116`.
pub fn wasm_opt_version_ok(line: &str) -> bool {
    line.split_whitespace()
        .skip_while(|w| *w != "version")
        .nth(1)
        == Some(WASM_OPT_VERSION)
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

/// `<directorio de compilación>/web`: lo que escribe `web-build`, lo que se pasa a
/// `install --stage --web` y lo que leerá el `build.rs` de B15.
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

/// Un nombre de directorio de hash: 12 dígitos hexadecimales en minúscula.
fn is_hash_dir(name: &str) -> bool {
    name.len() == 12 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Escribe los archivos de un crate en `<next>/<hash>/`.
fn write_built(next: &Path, b: &Built) -> Result<(), String> {
    let dir = next.join(&b.hash);
    fs::create_dir_all(&dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    for (_, physical, bytes) in &b.files {
        let p = dir.join(physical);
        fs::write(&p, bytes).map_err(|e| format!("no se pudo escribir {}: {e}", p.display()))?;
    }
    Ok(())
}

/// Copia (sin subir) los archivos regulares de `from` a `to`, que se crea.
fn copy_flat(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("no se pudo crear {}: {e}", to.display()))?;
    let rd = fs::read_dir(from).map_err(|e| format!("no se pudo leer {}: {e}", from.display()))?;
    for entry in rd {
        let entry = entry.map_err(|e| format!("no se pudo leer {}: {e}", from.display()))?;
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            let dest = to.join(entry.file_name());
            fs::copy(entry.path(), &dest)
                .map_err(|e| format!("no se pudo copiar a {}: {e}", dest.display()))?;
        }
    }
    Ok(())
}

/// Completa en `next` (que ya tiene los directorios de hash de `fresh`) la salida
/// nueva: une `fresh` sobre `old` (compilar un crate conserva los demás), copia de
/// `out` los directorios de hash de `old` que siguen vivos y escribe
/// `next/manifest.json`. Una entrada vieja cuyo directorio falta se descarta con
/// aviso. `out` no se toca: la salida es solo de `web-build`, así que lo que el
/// manifiesto no referencia no pasa a la nueva.
pub fn assemble(
    out: &Path,
    next: &Path,
    old: Manifest,
    fresh: Manifest,
) -> Result<Manifest, String> {
    let mut merged = Manifest::default();
    for (logical, path) in old.files {
        if fresh.files.contains_key(&logical) {
            continue;
        }
        let Some(hash) = path.split('/').next().filter(|h| is_hash_dir(h)) else {
            eprintln!("aviso: {logical} → {path} no tiene forma <hash>/…; se descarta");
            continue;
        };
        let have = next.join(hash);
        if !have.is_dir() {
            let from = out.join(hash);
            if !from.is_dir() {
                eprintln!(
                    "aviso: {logical} → {path} falta en {}; se descarta",
                    out.display()
                );
                continue;
            }
            copy_flat(&from, &have)?;
        }
        merged.files.insert(logical, path);
    }
    merged.files.extend(fresh.files);
    let text = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
    let path = next.join(MANIFEST_FILE);
    fs::write(&path, format!("{text}\n"))
        .map_err(|e| format!("no se pudo escribir {}: {e}", path.display()))?;
    Ok(merged)
}

/// Pone `next` en el lugar de `out`. Si `out` existe, `renameat2(RENAME_EXCHANGE)`
/// los intercambia de una vez y la salida vieja acaba en `trash`, que se borra; si el
/// sistema de archivos no lo admite, dos `rename` (con una ventana sin `out`).
pub fn swap_into_place(next: &Path, out: &Path, trash: &Path) -> Result<(), String> {
    use nix::fcntl::{AT_FDCWD, RenameFlags, renameat2};
    let err = |e: &dyn std::fmt::Display| format!("no se pudo instalar {}: {e}", out.display());
    if out.symlink_metadata().is_err() {
        return fs::rename(next, out).map_err(|e| err(&e));
    }
    let _ = fs::remove_dir_all(trash);
    match renameat2(AT_FDCWD, next, AT_FDCWD, out, RenameFlags::RENAME_EXCHANGE) {
        // Tras el intercambio, `next` es la salida vieja.
        Ok(()) => fs::rename(next, trash).map_err(|e| err(&e))?,
        Err(_) => {
            fs::rename(out, trash).map_err(|e| err(&e))?;
            fs::rename(next, out).map_err(|e| err(&e))?;
        }
    }
    let _ = fs::remove_dir_all(trash);
    Ok(())
}

/// Compila los crates de `opts` y, solo si todo salió bien, sustituye `<out>` por
/// la salida nueva con su `manifest.json`. Un fallo (cargo, bindgen, wasm-opt o
/// presupuesto) no crea ni toca `<out>`. Los temporales viven en
/// `<target>/web-build/`, fuera de `<out>`.
pub fn build(opts: &Options) -> Result<Manifest, String> {
    opts.tools.check()?;
    let built = opts
        .crates
        .iter()
        .map(|c| build_one(opts, c))
        .collect::<Result<Vec<_>, _>>()?;
    let path = opts.out.join(MANIFEST_FILE);
    let old = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("{} ilegible: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Manifest::default(),
        Err(e) => return Err(format!("no se pudo leer {}: {e}", path.display())),
    };
    let work = opts.target_dir.join("web-build");
    let pid = std::process::id();
    let next = work.join(format!("out.{pid}"));
    let _ = fs::remove_dir_all(&next);
    let result = (|| {
        fs::create_dir_all(&next)
            .map_err(|e| format!("no se pudo crear {}: {e}", next.display()))?;
        let mut fresh = Manifest::default();
        for b in &built {
            write_built(&next, b)?;
            for (logical, physical, _) in &b.files {
                fresh
                    .files
                    .insert(logical.clone(), format!("{}/{physical}", b.hash));
            }
        }
        let merged = assemble(&opts.out, &next, old, fresh)?;
        if let Some(parent) = opts.out.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("no se pudo crear {}: {e}", parent.display()))?;
        }
        swap_into_place(&next, &opts.out, &work.join(format!("old.{pid}")))?;
        Ok(merged)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&next);
    }
    result
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
