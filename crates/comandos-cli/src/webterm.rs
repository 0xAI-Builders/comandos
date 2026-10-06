//! Lifecycle of legacy ttyd instances; native listeners advertise exact ownership.
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Duration,
};
pub trait Runner {
    fn run(&self, program: &str, args: &[String]) -> io::Result<Output>;
    fn exec(&self, program: &str, args: &[String]) -> io::Error;
    fn available(&self, program: &str) -> bool;
    fn detach(&self, program: &str, args: &[String]) -> io::Result<u32>;
}
pub struct SystemRunner;
impl Runner for SystemRunner {
    fn run(&self, p: &str, a: &[String]) -> io::Result<Output> {
        Command::new(p).args(a).output()
    }
    fn exec(&self, p: &str, a: &[String]) -> io::Error {
        Command::new(p).args(a).exec()
    }
    fn available(&self, p: &str) -> bool {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|path| {
                std::fs::metadata(path.join(p))
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
        })
    }
    fn detach(&self, p: &str, a: &[String]) -> io::Result<u32> {
        Ok(Command::new(p)
            .args(a)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .id())
    }
}
pub trait Probe {
    fn get(&self, port: u16, path: &str) -> Option<Response>;
    fn wait(&self, port: u16, path: &str) -> bool {
        self.get(port, path).is_some()
    }
}
#[derive(Clone, Debug)]
pub struct Response {
    pub body: String,
    pub mode: Option<String>,
    pub ports: Vec<u16>,
}
pub struct HttpProbe;
impl Probe for HttpProbe {
    fn get(&self, port: u16, path: &str) -> Option<Response> {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let mut s = TcpStream::connect_timeout(&address, Duration::from_millis(400)).ok()?;
        s.set_read_timeout(Some(Duration::from_millis(400))).ok()?;
        s.set_write_timeout(Some(Duration::from_millis(400))).ok()?;
        write!(
            s,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .ok()?;
        let mut bytes = Vec::new();
        s.take(64 * 1024).read_to_end(&mut bytes).ok()?;
        let wire = String::from_utf8(bytes).ok()?;
        let (head, body) = wire.split_once("\r\n\r\n")?;
        if !head.lines().next()?.starts_with("HTTP/1.1 200 ") {
            return None;
        }
        let mut r = Response {
            body: body.into(),
            mode: None,
            ports: vec![],
        };
        for line in head.lines().skip(1) {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("x-comandos-term") {
                    r.mode = Some(value.trim().into());
                }
                if name.eq_ignore_ascii_case("x-comandos-term-compat") {
                    r.ports = value
                        .trim()
                        .split(',')
                        .filter_map(|p| p.parse().ok())
                        .collect();
                }
            }
        }
        Some(r)
    }
    fn wait(&self, port: u16, path: &str) -> bool {
        for _ in 0..20 {
            if self.get(port, path).is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}
pub struct Options {
    pub home: PathBuf,
    pub primary: u16,
    pub path: u16,
    pub dashboard: u16,
    pub attach: PathBuf,
    pub index: PathBuf,
}
impl Options {
    pub fn for_home(home: &Path) -> Self {
        Self {
            home: home.into(),
            primary: 4779,
            path: 4780,
            dashboard: 4777,
            attach: home.join(".local/bin/cc-webterm-attach"),
            index: home.join(".claude/hooks/dash/term.html"),
        }
    }
}
fn enabled(options: &Options) -> io::Result<()> {
    let file = options.home.join(".claude/hooks/webterm-enabled");
    std::fs::create_dir_all(
        file.parent()
            .ok_or_else(|| io::Error::other("enabled parent"))?,
    )?;
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&file)?;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
}
fn health(o: &Options, p: &dyn Probe) -> &'static str {
    match (
        p.get(o.primary, "/token").is_some(),
        p.get(o.path, "/term/token").is_some(),
    ) {
        (true, true) => "active",
        (false, false) => "off",
        _ => "degraded",
    }
}
fn stop(o: &Options, r: &dyn Runner) -> io::Result<()> {
    let _ = r.run(
        "systemctl",
        &[
            "--user".into(),
            "stop".into(),
            "cc-webterm.service".into(),
            "cc-webterm-path.service".into(),
        ],
    );
    let file = o.home.join(".claude/hooks/cc-webterm-rust-pids.json");
    if let Ok(bytes) = std::fs::read(&file)
        && let Ok(pids) = serde_json::from_slice::<Vec<(u32, u16)>>(&bytes)
    {
        for (pid, port) in pids {
            let cmd = r.run(
                "ps",
                &["-p".into(), pid.to_string(), "-o".into(), "args=".into()],
            );
            if cmd.is_ok_and(|out| {
                let text = String::from_utf8_lossy(&out.stdout);
                text.contains("ttyd")
                    && text.contains(o.attach.to_string_lossy().as_ref())
                    && text
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .windows(2)
                        .any(|w| w[0] == "-p" && w[1] == port.to_string())
            }) {
                let _ = r.run("kill", &[pid.to_string()]);
            }
        }
    }
    match std::fs::remove_file(file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
pub fn run_with(
    args: &[String],
    o: &Options,
    r: &dyn Runner,
    p: &dyn Probe,
    out: &mut dyn Write,
) -> io::Result<i32> {
    match args.first().map(String::as_str).unwrap_or("on") {
        "status" => {
            writeln!(out, "{}", health(o, p))?;
            return Ok(0);
        }
        "off" => {
            let file = o.home.join(".claude/hooks/webterm-enabled");
            match std::fs::remove_file(file) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            stop(o, r)?;
            writeln!(out, "\x1b[32m✓\x1b[0m Terminal web detenido.")?;
            return Ok(0);
        }
        "on" => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "uso: webterm [on|off|status]",
            ));
        }
    }
    if health(o, p) == "active" {
        enabled(o)?;
        writeln!(out, "\x1b[32m✓\x1b[0m Terminal web ya esta activo.")?;
        return Ok(0);
    }
    let front = p.get(o.dashboard, "/term/token").filter(|v| {
        v.body == r#"{"token":""}"# && matches!(v.mode.as_deref(), Some("ttyd" | "native"))
    });
    let owned = |port: u16| front.as_ref().is_some_and(|f| f.ports.contains(&port));
    let launch = [
        (o.primary, false, "cc-webterm"),
        (o.path, true, "cc-webterm-path"),
    ]
    .into_iter()
    .filter(|(port, _, _)| !owned(*port))
    .collect::<Vec<_>>();
    if !launch.is_empty() && !r.available("ttyd") {
        writeln!(
            out,
            "Falta 'ttyd' (el motor del terminal web). Instalalo:\n  Linux (Debian/Ubuntu):  sudo apt install ttyd\n  Linux (Fedora):         sudo dnf install ttyd\n  macOS:                  brew install ttyd\n  Windows:                usa WSL y el comando de Ubuntu"
        )?;
        return Ok(1);
    }
    comandos_server::dash::load_token(&o.home)?;
    if !launch.is_empty() {
        stop(o, r)?;
    }
    let help = if launch.is_empty() {
        String::new()
    } else {
        r.run("ttyd", &["--help".into()])
            .map(|v| {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&v.stdout),
                    String::from_utf8_lossy(&v.stderr)
                )
            })
            .unwrap_or_default()
    };
    let mut common = vec!["-i".into(), "127.0.0.1".into()];
    if help.contains("--writable") {
        common.push("-W".into())
    }
    common.push("--url-arg".into());
    for pref in [
        r##"theme={"background":"#0A0D13","foreground":"#EAF0FB","cursor":"#FFAE1A","selectionBackground":"#2E3852"}"##,
        "fontSize=11",
        "fontFamily=Ubuntu Sans Mono, JetBrainsMono Nerd Font Mono, JetBrainsMono Nerd Font, JetBrains Mono, DejaVu Sans Mono, monospace",
        "rendererType=canvas",
        "scrollback=10000",
        "cursorBlink=true",
        "disableLeaveAlert=true",
        "disableResizeOverlay=true",
    ] {
        common.extend(["-t".into(), pref.into()]);
    }
    common.push(o.attach.to_string_lossy().into_owned());
    let mut pids = Vec::new();
    for (port, path, unit) in launch {
        let mut args = vec!["-p".into(), port.to_string()];
        if path {
            args.extend(["-b".into(), "/term".into()]);
            if help.contains("--index") && o.index.is_file() {
                args.extend(["-I".into(), o.index.to_string_lossy().into_owned()]);
            }
        }
        args.extend(common.clone());
        if r.available("systemd-run") {
            let _ = r.run(
                "systemctl",
                &[
                    "--user".into(),
                    "reset-failed".into(),
                    format!("{unit}.service"),
                ],
            );
            let mut command = vec![
                "--user".into(),
                "--collect".into(),
                "--quiet".into(),
                format!("--unit={unit}"),
                "--property=Restart=on-failure".into(),
                "--property=RestartSec=1s".into(),
                "ttyd".into(),
            ];
            command.extend(args);
            if !r.run("systemd-run", &command)?.status.success() {
                return Ok(1);
            }
        } else {
            let detach = if r.available("setsid") {
                "setsid"
            } else {
                "nohup"
            };
            let mut command = vec!["ttyd".into()];
            command.extend(args);
            pids.push((r.detach(detach, &command)?, port));
            if !pids.is_empty() {
                let file = o.home.join(".claude/hooks/cc-webterm-rust-pids.json");
                let mut handle = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .mode(0o600)
                    .open(file)?;
                handle.write_all(&serde_json::to_vec(&pids)?)?;
            }
        }
    }
    if p.wait(o.primary, "/token") && p.wait(o.path, "/term/token") {
        enabled(o)?;
        writeln!(
            out,
            "\x1b[32m✓\x1b[0m Terminal web listo en 127.0.0.1:{} (:8443) y 127.0.0.1:{} (/term).\n  Exponlo al celular con:  cc-mobile   (lo enruta por https del tailnet)\n  (Solo tu tailnet lo alcanza — como SSH sobre Tailscale)",
            o.primary, o.path
        )?;
        Ok(0)
    } else {
        writeln!(
            out,
            "ttyd no arranco por completo. Estado: {}",
            health(o, p)
        )?;
        Ok(1)
    }
}
pub fn main(args: &[String]) -> i32 {
    let Some(home) = std::env::var_os("HOME").filter(|s| !s.is_empty()) else {
        eprintln!("HOME no está definido");
        return 1;
    };
    let mut options = Options::for_home(Path::new(&home));
    for (name, target) in [
        ("CC_WEBTERM_PORT", &mut options.primary),
        ("CC_WEBTERM_PATH_PORT", &mut options.path),
    ] {
        if let Ok(value) = std::env::var(name) {
            match value.parse::<u16>() {
                Ok(port) if port != 0 => *target = port,
                _ => {
                    eprintln!("{name}: puerto no válido");
                    return 2;
                }
            }
        }
    }
    run_with(
        args,
        &options,
        &SystemRunner,
        &HttpProbe,
        &mut io::stdout().lock(),
    )
    .unwrap_or_else(|e| {
        eprintln!("comandos webterm: {e}");
        1
    })
}
