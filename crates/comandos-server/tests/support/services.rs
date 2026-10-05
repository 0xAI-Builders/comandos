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

// ---------------------------------------------------------------------------
// Remoto y terminal web (Tarea 2)
// ---------------------------------------------------------------------------

/// `tailscale` falso de los dos lados del gemelo (el real nunca corre). Anota
/// cada llamada en `<HOME>/tailscale.log` (formato de `oracle::calls_in`) y
/// responde según el guion de `<HOME>/tailscale/`:
/// - `status.json`: salida de `status --json` (sin archivo → rc 1);
/// - `self.txt`: salida de `status --self --json` (rc 0 siempre);
/// - `logged-in`: `status` con rc 0 (sin él, «Logged out.» y rc 1);
/// - `serve.txt`: salida de `serve status`; un `serve … <destino>` que
///   funciona le añade `|-- proxy <destino>` y `off`/`reset` lo vacían;
/// - `serve-fail`: todo `serve` que no sea `status`/`off`/`reset` escribe su
///   contenido en stderr y sale con 1.
pub const TAILSCALE: &str = r#"#!/bin/sh
printf '%s\0' tailscale "$@" "$(printf '\036')" >> "$HOME/tailscale.log"
d="$HOME/tailscale"
case "$*" in
  "status --json")
    [ -f "$d/status.json" ] || exit 1
    cat "$d/status.json"; exit 0 ;;
  "status --self --json")
    [ -f "$d/self.txt" ] && cat "$d/self.txt"; exit 0 ;;
  "status")
    [ -f "$d/logged-in" ] && exit 0
    echo "Logged out." >&2; exit 1 ;;
  "serve status")
    [ -f "$d/serve.txt" ] && cat "$d/serve.txt"; exit 0 ;;
  "serve reset"|"serve --https=443 off"|"serve --https=8443 off")
    : > "$d/serve.txt"; exit 0 ;;
  serve\ *)
    if [ -f "$d/serve-fail" ]; then cat "$d/serve-fail" >&2; exit 1; fi
    for a in "$@"; do last=$a; done
    printf '|-- proxy %s\n' "$last" >> "$d/serve.txt"; exit 0 ;;
esac
echo "tailscale falso: orden desconocida: $*" >&2
exit 2
"#;

/// `cc-webterm` falso: anota en `<HOME>/fakebin.log` como los demás falsos y,
/// si existe `<HOME>/webterm-fail`, escribe su contenido en stderr y sale con 3.
pub const CC_WEBTERM: &str = r#"#!/bin/sh
printf '%s\0' cc-webterm "$@" "$(printf '\036')" >> "$HOME/fakebin.log"
if [ -f "$HOME/webterm-fail" ]; then cat "$HOME/webterm-fail" >&2; exit 3; fi
exit 0
"#;

/// Ejecutables extra del gemelo del remoto (los dos lados).
pub fn remote_fakes() -> Vec<(String, String)> {
    vec![
        ("tailscale".into(), TAILSCALE.into()),
        ("cc-webterm".into(), CC_WEBTERM.into()),
    ]
}

/// Escribe (o borra, con `None`) un archivo del guion de `tailscale` de un HOME.
pub fn tailscale_set(home: &TestHome, name: &str, text: Option<&str>) {
    let dir = home.root.join("tailscale");
    std::fs::create_dir_all(&dir).unwrap();
    match text {
        Some(text) => std::fs::write(dir.join(name), text).unwrap(),
        None => {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
}

/// Llamadas al `tailscale` falso de un HOME (solo los argumentos).
pub fn tailscale_log(home: &TestHome) -> Vec<Vec<String>> {
    super::oracle::calls_in(&home.root.join("tailscale.log"))
        .into_iter()
        .map(|call| call.args)
        .collect()
}

/// Llamadas a un falso del `fakebin` de un HOME (solo los argumentos).
pub fn calls_of(home: &TestHome, name: &str) -> Vec<Vec<String>> {
    calls(home)
        .into_iter()
        .filter(|(n, _)| n == name)
        .map(|(_, args)| args)
        .collect()
}
