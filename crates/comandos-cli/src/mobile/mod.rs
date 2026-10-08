//! Tailnet-only access to the single dashboard/terminal front, with exact Node backups.
mod backup;
mod config;
mod process;
use comandos_server::dash::token::{base64_urlsafe, token_path};
use comandos_store::domains::DomainStore;
use serde_json::Value;
use std::{
    env,
    ffi::OsStr,
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};
const TIMEOUT: Duration = Duration::from_secs(10);
const HELP: &str = "uso: comandos mobile [on|off|status] [--dry-run]\n     comandos mobile rollback RUTA_ABSOLUTA_BACKUP [--dry-run]\n";
enum Action {
    On,
    Off,
    Status,
    Rollback(PathBuf),
}
struct Options {
    home: PathBuf,
    action: Action,
    dry: bool,
}
fn parse(args: &[String]) -> Option<Options> {
    let mut dry = false;
    let mut words = Vec::new();
    for word in args {
        if word == "--dry-run" {
            if dry {
                return None;
            }
            dry = true;
        } else {
            words.push(word.as_str());
        }
    }
    let action = match words.as_slice() {
        [] | ["on"] => Action::On,
        ["off"] => Action::Off,
        ["status"] => Action::Status,
        ["rollback", p] if Path::new(p).is_absolute() => Action::Rollback(PathBuf::from(p)),
        _ => return None,
    };
    let home = PathBuf::from(env::var_os("HOME")?);
    if !home.is_absolute() {
        return None;
    }
    Some(Options { home, action, dry })
}
fn which(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join(name))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}
fn run(tool: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Output, String> {
    let args = args.iter().map(OsStr::new).collect::<Vec<_>>();
    process::run(tool, &args, input, TIMEOUT)
}
fn checked(tool: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let out = run(tool, args, input)?;
    if !out.status.success() {
        return Err(format!(
            "{}: exit {}; {}",
            tool.display(),
            out.status
                .code()
                .map_or_else(|| "signal".into(), |n| n.to_string()),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}
fn node(ts: &Path) -> Result<(Vec<u8>, Value), String> {
    let bytes = checked(ts, &["serve", "status", "--json"], None)?;
    let parsed = config::parse(&bytes)?;
    Ok((bytes, parsed))
}
fn authority(home: &Path) -> Result<(), String> {
    // dash-token is a configuration exception in the catalog, not a UI document.
    // This API validates future schemas, durable seals and source read guards;
    // it creates neither SQLite nor a mode lock and does not invent a domain.
    if comandos_store::domains::catalog::source("H/dash-token").is_some()
        || comandos_store::domains::catalog::file_classification("H/dash-token")
            != "se-queda-como-archivo"
    {
        return Err("dash-token fuera de la excepción de configuración del catálogo".into());
    }
    DomainStore { home }
        .modes_readonly()
        .map(|_| ())
        .map_err(|e| format!("autoridad de estado: {e}"))
}
fn existing_token(home: &Path) -> Result<Option<String>, String> {
    let path = token_path(home);
    backup::parents(&path)?;
    match path.symlink_metadata() {
        Ok(_) => {
            let raw = backup::private_regular(&path)?;
            let text = String::from_utf8(raw).map_err(|_| "dash-token no es UTF-8".to_owned())?;
            let text = comandos_core::text::strip(&text);
            Ok((!text.is_empty()).then(|| text.to_owned()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
fn token(home: &Path) -> Result<String, String> {
    authority(home)?;
    let existing = existing_token(home)?;
    finish_token(home, existing)
}
fn finish_token(home: &Path, existing: Option<String>) -> Result<String, String> {
    if let Some(token) = existing {
        // Publish the bytes actually validated, without a pathname reopen.
        return Ok(token);
    }
    use std::{
        io::{Read, Seek, SeekFrom, Write},
        os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    };
    let path = token_path(home);
    backup::parents(&path)?;
    let hooks = path.parent().ok_or("token sin directorio")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(hooks)
        .map_err(|e| format!("{}: {e}", hooks.display()))?;
    let flags = nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY;
    let opened = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(flags)
        .open(&path);
    let file = match opened {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(flags)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?,
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut file = nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock)
        .map_err(|(_, e)| e.to_string())?;
    let held = file.metadata().map_err(|e| e.to_string())?;
    if !held.is_file() || held.permissions().mode() & 0o7777 != 0o600 {
        return Err("dash-token requiere archivo regular 0600".into());
    }
    let still_owned = || -> Result<(), String> {
        let entry = path.symlink_metadata().map_err(|e| e.to_string())?;
        if !entry.is_file() || (entry.dev(), entry.ino()) != (held.dev(), held.ino()) {
            return Err("dash-token cambió durante generación".into());
        }
        backup::parents(&path)
    };
    still_owned()?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut *file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("dash-token supera 1 MiB".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "dash-token no es UTF-8".to_owned())?;
    let text = comandos_core::text::strip(&text);
    if !text.is_empty() {
        still_owned()?;
        return Ok(text.to_owned());
    }
    let mut random = [0; 32];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let token = base64_urlsafe(&random);
    still_owned()?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    file.set_len(0).map_err(|e| e.to_string())?;
    file.write_all(token.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    still_owned()?;
    Ok(token)
}
fn hostname(status: &Value, on: bool) -> Result<String, String> {
    if status.get("BackendState").and_then(Value::as_str) != Some("Running") {
        return Err("Tailscale no está conectado; inicia sesión antes de usar mobile".into());
    }
    let host = status
        .get("Self")
        .and_then(|v| v.get("DNSName"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim_end_matches('.');
    if host.is_empty()
        || host.len() > 253
        || !host.contains('.')
        || host.split('.').any(|s| {
            s.is_empty()
                || s.len() > 63
                || s.starts_with('-')
                || s.ends_with('-')
                || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err("No pude leer tu nombre de tailnet. Revisa: tailscale status".into());
    }
    if on
        && !status
            .get("CertDomains")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(host)))
    {
        return Err("Tailscale no ofrece certificados HTTPS para este nodo; habilita HTTPS en la tailnet antes de mobile (sin flujo interactivo automático)".into());
    }
    Ok(host.to_owned())
}
fn front() -> Result<(), String> {
    let curl = which("curl").ok_or("curl no está instalado")?;
    checked(
        &curl,
        &[
            "-fsS",
            "--max-time",
            "2",
            "--",
            "http://127.0.0.1:4777/prefs",
        ],
        None,
    )
    .map(|_| ())
    .map_err(|_| "cc-dash no responde en el puerto 4777. Arráncalo antes de mobile".into())
}
fn url(host: &str, token: &str) -> Result<String, String> {
    let mut url = url::Url::parse(&format!("https://{host}/")).map_err(|e| e.to_string())?;
    url.query_pairs_mut().append_pair("token", token);
    Ok(url.into())
}
fn quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"))
}
fn rollback_hint(path: &Path) -> String {
    format!("comandos mobile rollback {}", quote(path))
}
fn pairing(host: &str, token: &str) -> Result<(), String> {
    let url = url(host, token)?;
    println!(
        "\n\x1b[32m✓\x1b[0m Tablero expuesto a TU tailnet (cifrado, privado, con TLS).\nTerminal: https://{host}/term (WebSocket /term/ws del mismo frente).\n\n  Abre esto en tu celular (o escanea el QR):\n  \x1b[36m{url}\x1b[0m\n"
    );
    if let Some(qr) = which("qrencode") {
        match run(&qr, &["-t", "ANSIUTF8", &url], None) {
            Ok(out) if out.status.success() => {
                use std::io::Write;
                io::stdout()
                    .write_all(&out.stdout)
                    .map_err(|e| e.to_string())?;
            }
            _ => eprintln!("cc-mobile: no se pudo generar el QR; usa el enlace"),
        }
    } else {
        println!("  (Instala 'qrencode' para ver un QR aquí)");
    }
    println!(
        "\n  En el celular: abrelo en Safari/Chrome -> menu -> 'Agregar a inicio'\n  para instalarlo como app. El token queda guardado; no lo compartas.\n\n  Para dejar de exponerlo:  cc-mobile off"
    );
    Ok(())
}
fn status(ts: &Path) -> Result<(), String> {
    fn shown(ts: &Path, args: &[&str], lines: usize) -> Result<(), String> {
        let out = run(ts, args, None)?;
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines().take(lines) {
            println!("{line}");
        }
        if !out.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
        }
        if !out.status.success() {
            return Err("Tailscale no pudo consultar el estado".into());
        }
        Ok(())
    }
    println!("Tailscale:");
    shown(ts, &["status"], 3)?;
    println!("\nServe:");
    shown(ts, &["serve", "status"], 16)?;
    println!(
        "\nFrente único: {} (/term/ws en 4777)",
        if front().is_ok() { "active" } else { "off" }
    );
    Ok(())
}
fn ensure_current(ts: &Path, expected: &Value) -> Result<(), String> {
    let (_, current) = node(ts)?;
    if &current != expected {
        return Err(
            "Serve cambió durante la operación; no se sobrescribe configuración ajena".into(),
        );
    }
    Ok(())
}
fn apply(ts: &Path, path: &Path, bytes: &[u8], expected: &Value) -> Result<(), String> {
    checked(ts, &["serve", "set-raw"], Some(bytes))
        .map_err(|e| format!("{e}; rollback: {}", rollback_hint(path)))?;
    ensure_current(ts, expected).map_err(|e| {
        format!(
            "no se pudo verificar Serve: {e}; rollback: {}",
            rollback_hint(path)
        )
    })
}
fn rollback(options: &Options, ts: &Path, path: &Path) -> Result<(), String> {
    let restore = backup::restore(&options.home, path)?;
    let (_, current) = node(ts)?;
    config::safe_to_change(&current)?;
    if current == restore.before_config {
        println!("Serve ya coincide con el backup {}", path.display());
        return Ok(());
    }
    if current != restore.after {
        return Err(
            "Serve cambió después del backup; rollback rechazado para preservar otros servicios"
                .into(),
        );
    }
    if options.dry {
        println!(
            "dry-run: restaurar NodeServeConfig exacto desde {}; rollback: {}",
            path.display(),
            rollback_hint(path)
        );
        return Ok(());
    }
    ensure_current(ts, &restore.after)?;
    apply(ts, path, &restore.before, &restore.before_config)?;
    println!("NodeServeConfig restaurado desde {}", path.display());
    Ok(())
}
fn execute(options: Options) -> Result<(), String> {
    let ts=which("tailscale").ok_or("Tailscale no esta instalado. Instalalo desde https://tailscale.com/download y vuelve a correr cc-mobile.")?;
    if matches!(options.action, Action::Status) {
        return status(&ts);
    }
    if let Action::Rollback(path) = &options.action {
        return rollback(&options, &ts, path);
    }
    let on = matches!(options.action, Action::On);
    if on && env::var_os("CC_PORT").is_some_and(|v| v != "4777") {
        return Err("mobile sólo admite el frente único en 127.0.0.1:4777".into());
    }
    let st: Value = serde_json::from_slice(&checked(&ts, &["status", "--json"], None)?)
        .map_err(|e| format!("estado Tailscale inválido: {e}"))?;
    let host = hostname(&st, on)?;
    let (raw, before) = node(&ts)?;
    let after = config::plan(&before, &host, on)?;
    let planned = serde_json::to_vec(&after).map_err(|e| e.to_string())?;
    if planned.len() > 1024 * 1024 {
        return Err("plan Serve supera 1 MiB; no se modifica".into());
    }
    let old_token = if on {
        authority(&options.home)?;
        existing_token(&options.home)?
    } else {
        None
    };
    let path = backup::destination(&options.home)?;
    if on {
        front()?;
    }
    if options.dry {
        println!(
            "dry-run: {} sólo / → {} con TLS tailnet 443; backend terminal /term/ws en el mismo frente",
            if on {
                "publicar"
            } else {
                "quitar rutas propias"
            },
            config::FRONT
        );
        if before != after {
            println!(
                "backup exacto: {}; rollback: {}",
                path.display(),
                rollback_hint(&path)
            );
        } else {
            println!("dry-run: Serve ya coincide; no requiere backup ni cambio de Serve");
        }
        if on && old_token.is_none() {
            println!(
                "dry-run: dash-token se generaría mediante la API del servidor; no se escribe"
            );
        }
        return Ok(());
    }
    if before != after {
        ensure_current(&ts, &before)?;
        backup::save(&path, &raw, &after)?;
    }
    let token = if on {
        Some(token(&options.home)?)
    } else {
        None
    };
    if before != after {
        ensure_current(&ts, &before).map_err(|e| format!("{e}; backup: {}", path.display()))?;
        apply(&ts, &path, &planned, &after)?;
        println!(
            "backup: {}; rollback: {}",
            path.display(),
            rollback_hint(&path)
        );
    }
    if let Some(token) = token {
        pairing(&host, &token)?;
    } else {
        println!(
            "\x1b[32m✓\x1b[0m Tablero y terminal ya NO estan expuestos por las rutas propias de mobile. Otros servicios conservados."
        );
    }
    Ok(())
}
pub fn main(args: &[String]) -> i32 {
    if args == ["--help"] || args == ["help"] {
        print!("{HELP}");
        return 0;
    }
    let Some(options) = parse(args) else {
        eprint!("{HELP}");
        return 2;
    };
    match execute(options) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("\x1b[31m✗\x1b[0m {e}");
            1
        }
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, symlink};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "mobile-token-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
            let f = Self(p);
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(f.path().parent().unwrap())
                .unwrap();
            f
        }
        fn path(&self) -> PathBuf {
            token_path(&self.0)
        }
        fn put(&self, value: &[u8]) {
            fs::write(self.path(), value).unwrap();
            fs::set_permissions(self.path(), fs::Permissions::from_mode(0o600)).unwrap();
        }
        fn replace_with_symlink(&self) -> PathBuf {
            let outside = self.0.join("own-outside");
            fs::write(&outside, b"own-outside-sentinel").unwrap();
            if self.path().exists() {
                fs::remove_file(self.path()).unwrap();
            }
            symlink(&outside, self.path()).unwrap();
            outside
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn validated_token_is_not_reopened_after_path_replacement() {
        let f = Fixture::new();
        f.put(b"validated-own-token\n");
        let validated = existing_token(&f.0).unwrap();
        let outside = f.replace_with_symlink();
        assert_eq!(
            finish_token(&f.0, validated).unwrap(),
            "validated-own-token"
        );
        assert_eq!(fs::read(outside).unwrap(), b"own-outside-sentinel");
    }
    #[test]
    fn absent_or_empty_token_replacement_cannot_write_through_a_symlink() {
        for empty in [false, true] {
            let f = Fixture::new();
            if empty {
                f.put(b" \n");
            }
            let validated = existing_token(&f.0).unwrap();
            assert!(validated.is_none());
            let outside = f.replace_with_symlink();
            assert!(finish_token(&f.0, validated).is_err());
            assert_eq!(fs::read(outside).unwrap(), b"own-outside-sentinel");
            assert!(f.path().symlink_metadata().unwrap().is_symlink());
        }
    }
    #[test]
    fn generation_preserves_token_encoding_and_private_mode_without_database() {
        for empty in [false, true] {
            let f = Fixture::new();
            if empty {
                f.put(b" \n");
            }
            let generated = token(&f.0).unwrap();
            assert_eq!(generated.len(), 43);
            assert!(
                generated
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            );
            assert_eq!(fs::read(f.path()).unwrap(), generated.as_bytes());
            assert_eq!(
                f.path().metadata().unwrap().permissions().mode() & 0o7777,
                0o600
            );
            assert!(!comandos_store::unified::unified_path(&f.0).exists());
        }
    }
    #[test]
    fn existing_token_reuse_preserves_inode_mode_mtime_and_bytes() {
        let f = Fixture::new();
        f.put(b"\xc2\xa0 validated-own-token\n");
        let stamp = || {
            let m = f.path().metadata().unwrap();
            (
                m.dev(),
                m.ino(),
                m.mode(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
            )
        };
        let before = stamp();
        let bytes = fs::read(f.path()).unwrap();
        assert_eq!(token(&f.0).unwrap(), "validated-own-token");
        assert_eq!(stamp(), before);
        assert_eq!(fs::read(f.path()).unwrap(), bytes);
    }
    #[test]
    fn generation_rejects_a_fifo_substituted_after_empty_validation() {
        let f = Fixture::new();
        f.put(b"");
        let validated = existing_token(&f.0).unwrap();
        fs::remove_file(f.path()).unwrap();
        nix::unistd::mkfifo(
            &f.path(),
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .unwrap();
        let begin = std::time::Instant::now();
        assert!(finish_token(&f.0, validated).is_err());
        assert!(begin.elapsed() < Duration::from_millis(100));
    }
    #[test]
    fn generation_fails_promptly_if_another_writer_holds_the_owned_token() {
        let f = Fixture::new();
        f.put(b"");
        let validated = existing_token(&f.0).unwrap();
        let _lock = nix::fcntl::Flock::lock(
            fs::File::open(f.path()).unwrap(),
            nix::fcntl::FlockArg::LockExclusive,
        )
        .unwrap();
        let begin = std::time::Instant::now();
        assert!(finish_token(&f.0, validated).is_err());
        assert!(begin.elapsed() < Duration::from_millis(100));
        assert_eq!(fs::read(f.path()).unwrap(), b"");
    }
}
