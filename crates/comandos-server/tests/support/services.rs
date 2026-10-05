//! Ayudas de prueba del corte `services` (2f-3): remoto, SSH, ajustes,
//! catálogos y push. Cada tarea del dominio añade aquí lo suyo.
//!
//! Ajustes (Tarea 1): los programas que lanza el tablero (`xdg-open`,
//! `wslview`, `pw-play`, `paplay`, `spd-say`, `piper`) son los falsos que
//! anotan de `oracle::confined_fakebin` (en `<HOME>/fakebin.log`, los dos
//! lados del gemelo): nunca se abre nada ni suena nada de verdad.
#![allow(dead_code)]
use super::{
    TestHome,
    oracle::{FakeCall, fake_calls},
};
use regex::Regex;
use std::{
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

/// `cc-notify.conf` de partida: comentario, espacios alrededor del `=`,
/// popups apagados (sin red) y un sonido que existe en toda máquina.
pub const SERVICES_CONF: &str = "# ajustes de prueba\nDESKTOP_NOTIFY=0\nVOLUME = 70\n\
    SOUND_DONE=/bin/sh\nSOUND_ATTENTION=/no-existe/sonido.oga\nOTRA = x\n";

/// Siembra del gemelo de ajustes: `cc-notify.conf`, `~/codebase/{a,B,.oculto}`
/// y un archivo `~/codebase/archivo` (para `/fs/mkdir` sobre un archivo).
pub fn seed_services(home: &TestHome) {
    std::fs::write(home.hooks().join("cc-notify.conf"), SERVICES_CONF).unwrap();
    for dir in ["a", "B", ".oculto", "nuestra"] {
        std::fs::create_dir_all(home.root.join("codebase").join(dir)).unwrap();
    }
    std::fs::write(home.root.join("codebase/archivo"), "x").unwrap();
}

/// Rutas de cada HOME (tal cual y canónica) sustituidas por `<HOME>`, y el
/// nombre aleatorio de `mktemp --suffix=.wav` por uno fijo.
pub fn unhome(home: &TestHome, text: &str) -> String {
    static WAV: OnceLock<Regex> = OnceLock::new();
    let mut out = text.to_owned();
    if let Ok(real) = std::fs::canonicalize(&home.root) {
        out = out.replace(&real.display().to_string(), "<HOME>");
    }
    out = out.replace(&home.root.display().to_string(), "<HOME>");
    let wav = WAV.get_or_init(|| Regex::new(r"tmp\.[A-Za-z0-9]+\.wav").unwrap());
    wav.replace_all(&out, "tmp.X.wav").into_owned()
}

/// Llamadas a los falsos de un HOME con sus rutas normalizadas.
pub fn calls(home: &TestHome) -> Vec<(String, Vec<String>)> {
    fake_calls(&home.root)
        .into_iter()
        .map(|FakeCall { name, args }| {
            (
                name,
                args.iter().map(|a| unhome(home, a)).collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// Los hijos sueltos anotan después de la respuesta: espera hasta que los dos
/// registros coincidan y lleven `expected` llamadas (o vence el plazo).
pub async fn settle_calls(a: &TestHome, b: &TestHome, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (ca, cb) = (calls(a), calls(b));
        if (ca == cb && ca.len() >= expected) || Instant::now() > deadline {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Modo de un archivo (`-` si no existe).
pub fn mode(path: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| format!("{:o}", m.permissions().mode() & 0o7777))
        .unwrap_or_else(|_| "-".into())
}
