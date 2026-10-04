//! Lo lento del bash (`( notify_voice ) &` y `( notify_desktop ) &`) corre en un
//! proceso hijo desacoplado (`comandos hook __deliver`) con stdio a `/dev/null`,
//! para que el harness no espere: el hook regresa en milisegundos. El hijo hace,
//! en paralelo, la contabilidad de uso (el `cc_usage.py ... &` del bash), el POST
//! a cc-notifyd (con `osascript` si falla, como el bash) y la voz o el chime.
use super::state_file::mktemp;
use super::which;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Nombre interno del subcomando del proceso de entrega.
pub const WORKER: &str = "__deliver";
const DEFAULT_NOTIFYD: &str = "http://127.0.0.1:4778";

/// Voz (`speak`) o chime (`sound`) ya decididos por el hook.
pub struct Voice {
    pub speak: Option<Vec<u8>>,
    pub piper_model: PathBuf,
    pub spd_lang: &'static str,
    pub sound: Option<Vec<u8>>,
}

/// Popup de cc-notifyd: cuerpo JSON exacto y el script de `osascript` de respaldo.
pub struct Desktop {
    pub payload: String,
    pub osa_script: Vec<u8>,
}

fn b64(bytes: &[u8]) -> Value {
    Value::String(STANDARD.encode(bytes))
}

fn unb64(value: &Value) -> Option<Vec<u8>> {
    STANDARD.decode(value.as_str()?).ok()
}

/// Todo lo que el hook deja en segundo plano.
pub struct Job {
    pub home: PathBuf,
    /// Pasos de `usage_hook::step`, en orden.
    pub usage: Vec<Value>,
    pub volume: u32,
    pub voice: Option<Voice>,
    pub desktop: Option<Desktop>,
}

/// Lanza el proceso de entrega sin esperarlo; si nada que entregar, no lanza nada.
// El hijo debe sobrevivir al hook (lo adopta init), como los `&` del bash.
#[allow(clippy::zombie_processes)]
pub fn spawn(job: Job) {
    let Job {
        home,
        usage,
        volume,
        voice,
        desktop,
    } = job;
    if usage.is_empty() && voice.is_none() && desktop.is_none() {
        return;
    }
    let job = json!({
        "home": b64(home.as_os_str().as_bytes()),
        "usage": usage,
        "volume": volume,
        "voice": voice.map(|v| json!({
            "speak": v.speak.as_deref().map(b64),
            "piperModel": b64(v.piper_model.as_os_str().as_bytes()),
            "spdLang": v.spd_lang,
            "sound": v.sound.as_deref().map(b64),
        })),
        "desktop": desktop.map(|d| json!({"payload": d.payload, "osa": b64(&d.osa_script)})),
    });
    let Some(exe) = worker_exe() else {
        return;
    };
    let child = Command::new(exe)
        .args(["hook", WORKER])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child
        && let Some(mut stdin) = child.stdin.take()
    {
        let _ = stdin.write_all(job.to_string().as_bytes());
    }
}

/// Ejecutable del proceso de entrega: este mismo binario. En Linux, `/proc/self/exe`, que
/// sigue apuntando al inodo en ejecución aunque `install --stage` ya haya sustituido (o
/// borrado) el archivo; `current_exe()` daría entonces `… (deleted)`, que no se puede lanzar.
fn worker_exe() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        return Some(PathBuf::from("/proc/self/exe"));
    }
    std::env::current_exe().ok()
}

/// Cuerpo del proceso de entrega: uso, voz y popup en paralelo, como los `&`.
pub fn worker_main() -> i32 {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return 1;
    }
    let Ok(job) = serde_json::from_str::<Value>(&input) else {
        return 1;
    };
    let volume = job["volume"].as_u64().unwrap_or(60).min(100) as u32;
    let voice = job["voice"].clone();
    let desktop = job["desktop"].clone();
    let home = PathBuf::from(OsString::from_vec(unb64(&job["home"]).unwrap_or_default()));
    let usage = job["usage"].as_array().cloned().unwrap_or_default();
    let usage_thread = std::thread::spawn(move || super::usage_hook::apply(&home, &usage));
    let voice_thread = std::thread::spawn(move || {
        if voice.is_object() {
            run_voice(&voice, volume);
        }
    });
    if let Some(payload) = desktop["payload"].as_str() {
        let base = std::env::var("COMANDOS_NOTIFYD_URL").unwrap_or_else(|_| DEFAULT_NOTIFYD.into());
        if !post(&base, payload.to_owned())
            && let (Some(osascript), Some(script)) = (which("osascript"), unb64(&desktop["osa"]))
        {
            quiet(
                Command::new(osascript)
                    .arg("-e")
                    .arg(OsString::from_vec(script)),
            );
        }
    }
    let _ = voice_thread.join();
    let _ = usage_thread.join();
    0
}

/// Ejecuta y espera, con stdio a `/dev/null`; `true` si salió con éxito.
fn quiet(command: &mut Command) -> bool {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `notify_voice`: si hay frase, piper → say → spd-say; si no, el chime.
fn run_voice(voice: &Value, volume: u32) {
    if let Some(speak) = unb64(&voice["speak"]) {
        let model = PathBuf::from(OsString::from_vec(
            unb64(&voice["piperModel"]).unwrap_or_default(),
        ));
        if let Some(piper) = which("piper").filter(|_| model.is_file()) {
            let tmpdir = std::env::var_os("TMPDIR")
                .filter(|d| !d.is_empty())
                .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
            if let Some((wav, file)) = mktemp(&tmpdir, b"tmp.", 10, ".wav") {
                drop(file);
                let spoke = Command::new(piper)
                    .arg("-m")
                    .arg(&model)
                    .arg("-f")
                    .arg(&wav)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .and_then(|mut child| {
                        if let Some(mut stdin) = child.stdin.take() {
                            let _ = stdin.write_all(&speak);
                        }
                        child.wait()
                    })
                    .is_ok_and(|s| s.success());
                if spoke {
                    play(&wav, volume);
                }
                let _ = std::fs::remove_file(&wav);
            }
        } else if let Some(say) = which("say") {
            quiet(Command::new(say).arg(OsStr::from_bytes(&speak)));
        } else if let Some(spd) = which("spd-say") {
            let lang = voice["spdLang"].as_str().unwrap_or("en");
            let rate = (i64::from(volume) * 2 - 100).to_string();
            quiet(
                Command::new(spd)
                    .args(["-l", lang, "-i", &rate])
                    .arg(OsStr::from_bytes(&speak)),
            );
        }
    } else if let Some(sound) = unb64(&voice["sound"]) {
        play(Path::new(OsStr::from_bytes(&sound)), volume);
    }
}

/// `play_snd`: pw-play con volumen flotante (pipewire ignora el de paplay).
fn play(file: &Path, volume: u32) {
    let float = format!("{}.{:02}", volume / 100, volume % 100);
    if let Some(pw) = which("pw-play") {
        quiet(Command::new(pw).arg(format!("--volume={float}")).arg(file));
    } else if let Some(paplay) = which("paplay") {
        quiet(
            Command::new(paplay)
                .arg(format!("--volume={}", volume * 655))
                .arg(file),
        );
    } else if let Some(afplay) = which("afplay") {
        quiet(Command::new(afplay).args(["-v", &float]).arg(file));
    }
}

/// `curl -s -m 2 -X POST <base>/notify -H 'Content-Type: application/json' -d …`:
/// éxito es completar el intercambio HTTP, sea cual sea el código.
fn post(base: &str, payload: String) -> bool {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(2), send(base, payload))
            .await
            .is_ok_and(|r| r.is_some())
    })
}

async fn send(base: &str, payload: String) -> Option<()> {
    let rest = base.strip_prefix("http://")?;
    let (authority, prefix) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let address = if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    let stream = tokio::net::TcpStream::connect(address).await.ok()?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .ok()?;
    tokio::spawn(connection);
    let path = format!("/{}/notify", prefix.trim_matches('/')).replace("//", "/");
    let request = hyper::Request::post(path)
        .header(hyper::header::HOST, authority)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(payload)))
        .ok()?;
    let response = sender.send_request(request).await.ok()?;
    response.into_body().collect().await.ok()?;
    Some(())
}
