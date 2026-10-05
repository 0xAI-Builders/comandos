//! Ayudas de prueba del corte `tabs` (2f-1): pestañas, sesiones y teclas.
//!
//! Confinamiento: todo HOME de estas pruebas pasa por
//! `oracle::confined_fakebin` ANTES de arrancar su tmux, así que el servidor
//! privado y sus paneles nacen con el entorno confinado. El frente llama a
//! tmux por el guardián del `fakebin` (que anota en `<HOME>/tmux.log` y lleva
//! `-S` al socket privado, también en el prefijo del frente); el Python
//! (`oracle::run_dash`) llega al mismo guardián por su `PATH`. Ninguna ayuda
//! de aquí crea ni mata sesiones: la siembra usa `support::run_tmux` y la
//! limpieza es el `Drop` de `TestHome` (siempre por `-S`).
#![allow(dead_code)]
use super::{TestHome, oracle, twin};
use comandos_server::dash::native::{Native, NativeOptions};
use std::path::{Path, PathBuf};

/// Registro de órdenes de tmux de un HOME (`<HOME>/tmux.log` del guardián), sin
/// las opciones globales `-f`/`-S` del frente: comparable entre el frente y el
/// Python (E2 del plan).
pub struct TmuxLog<'a>(pub &'a TestHome);

impl TmuxLog<'_> {
    fn path(&self) -> PathBuf {
        self.0.root.join("tmux.log")
    }

    /// Todas las llamadas desde la última `take`/`clear`.
    pub fn read(&self) -> Vec<Vec<String>> {
        twin::tmux_log(self.0)
    }

    /// Las llamadas desde la última vez, y vacía el registro.
    pub fn take(&self) -> Vec<Vec<String>> {
        let calls = self.read();
        self.clear();
        calls
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(self.path());
    }
}

/// Opciones del frente sobre `home` con tmux = el guardián del `fakebin`
/// (registro en `<HOME>/tmux.log`, `-S` al socket privado en el prefijo).
pub fn options_for(home: &TestHome) -> NativeOptions {
    let fakebin = home.root.join("fakebin");
    assert!(
        fakebin.join(".confined").exists(),
        "options_for necesita el fakebin confinado (seed_registry)"
    );
    let mut opts = home.options();
    opts.tmux.program.path = fakebin.join("tmux");
    opts.search_path = Some(fakebin.into_os_string());
    opts.user_bin_dirs = twin::home_bin_dirs();
    super::assert_private_tmux(&opts);
    opts
}

/// Un `Native` sin servidor sobre `home` (`options_for`).
pub async fn native_for(home: &TestHome) -> Native {
    Native::new(options_for(home))
}

/// Siembra de las pruebas del registro: `fakebin` confinado, `app-tabs.json`
/// con `local` y `otra`, `app-tabs-meta.json` con una entrada válida, una de
/// tipo desconocido y otra que no es objeto, tres entradas de historial,
/// `~/codebase/p2f/` y `~/.ssh/config` (del HOME temporal) con `Host x`.
pub fn seed_registry(home: &TestHome) {
    oracle::confined_fakebin(home, &[]);
    home.write("app-tabs.json", r#"{"local": "local", "otra": "Otra"}"#);
    home.write(
        "app-tabs-meta.json",
        r#"{"otra": {"kind": "shell", "cwd": "/srv", "host": "", "x": 1}, "rara": {"kind": "nave"}, "mala": "texto"}"#,
    );
    home.write(
        "app-tabs-history.json",
        r#"[{"session": "vieja", "label": "Vieja", "cwd": "/v", "agent": "codex", "ts": 10, "reason": "closed"}, {"session": "p2f", "label": "P antes", "ts": 9}, {"session": "mal sesion", "ts": 8}]"#,
    );
    std::fs::create_dir_all(home.root.join("codebase/p2f")).unwrap();
    std::fs::create_dir_all(home.root.join(".ssh")).unwrap();
    std::fs::write(
        home.root.join(".ssh/config"),
        "Host x\n  HostName 10.0.0.1\n  User u\n\nHost *\n  User nadie\n",
    )
    .unwrap();
}

/// Texto de `<hooks>/<name>` (o `None`) comparable entre dos HOME: reloj
/// normalizado (`twin::normalize`) y la raíz del HOME como `~`.
pub fn read_normalized(home: &TestHome, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(home.hooks().join(name)).ok()?;
    Some(normalize_home(home, &twin::normalize(&text)))
}

/// La raíz del HOME (y su forma canónica) como `~`.
pub fn normalize_home(home: &TestHome, text: &str) -> String {
    let mut out = text.to_owned();
    let canonical = std::fs::canonicalize(&home.root).unwrap_or_else(|_| home.root.clone());
    for root in [canonical, home.root.clone()] {
        out = out.replace(&root.display().to_string(), "~");
    }
    out
}

/// Los archivos que escriben las funciones del registro.
pub const REGISTRY_FILES: [&str; 5] = [
    "app-tabs.json",
    "app-tabs-meta.json",
    "app-tabs-history.json",
    "app-tab-open.json",
    "app-tab-close.json",
];

/// Ruta de `<hooks>/<name>`.
pub fn hook_path(home: &TestHome, name: &str) -> PathBuf {
    home.hooks().join(name)
}

/// ¿Existe `<hooks>/<name>`?
pub fn exists(home: &TestHome, name: &str) -> bool {
    Path::new(&hook_path(home, name)).exists()
}
