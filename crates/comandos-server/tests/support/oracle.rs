//! El `cc-dash` Python del repositorio sobre el MISMO HOME que el frente:
//! se comparan bytes y se comprueba que lo que escribe uno lo lee el otro.
//! Sin `python3` las pruebas que lo usan se saltan con un aviso.
#![allow(dead_code)]
use super::TestHome;
use std::{
    ffi::OsStr,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::time::sleep;

pub struct Oracle {
    pub port: u16,
    child: Child,
    /// El oráculo confinado es líder de su grupo: al soltarlo muere el grupo
    /// entero (lo que haya lanzado sin `start_new_session`).
    group: bool,
}

impl Oracle {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        if self.group
            && let Ok(pid) = i32::try_from(self.child.id())
        {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Los ejecutables con efectos fuera del HOME de la prueba (terminal web,
/// tailscale, systemd, sonido, ventanas) como enlaces a `true` en `fakebin`,
/// igual que `xtask parity`; `ssh -O check` nunca alcanza el ssh real ni sus
/// sockets de control: falla siempre, como el `/no-existe/ssh` de
/// `TestHome::options`. El `tmux` lo pone cada llamador.
pub fn fake_effects(fakebin: &Path) {
    std::fs::create_dir_all(fakebin).unwrap();
    for name in [
        "systemctl",
        "wmctrl",
        "cc-webterm",
        "cc-webterm-attach",
        "systemd-run",
        "tailscale",
        "notify-send",
        "pw-play",
        "paplay",
        "spd-say",
        "piper",
        "xdg-open",
    ] {
        let link = fakebin.join(name);
        if !link.exists() {
            std::os::unix::fs::symlink("/bin/true", &link).unwrap();
        }
    }
    let ssh = fakebin.join("ssh");
    if !ssh.exists() {
        std::os::unix::fs::symlink("/bin/false", &ssh).unwrap();
    }
}

/// `tmux` como enlace a `true`: el Python nunca alcanza ningún servidor.
pub fn fake_tmux_true(fakebin: &Path) {
    let link = fakebin.join("tmux");
    if !link.exists() {
        std::os::unix::fs::symlink("/bin/true", &link).unwrap();
    }
}

/// Claves del entorno que cambian lo que calcula el Python de uso (D7 del plan 2e):
/// el lado Rust las recibe por parámetro, así que el oráculo no debe heredarlas.
pub const D7_KEYS: &[&str] = &[
    "COMANDOS_DAILY_BUDGET_USD",
    "COMANDOS_USAGE_DAILY_BUDGET_USD",
    "COMANDOS_CODEX_DAILY_TOKEN_LIMIT",
    "COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_WEEKLY_TOKEN_LIMIT",
    "CODEX_DAILY_TOKEN_LIMIT",
    "CODEX_WEEKLY_TOKEN_LIMIT",
    "CLAUDE_DAILY_TOKEN_LIMIT",
    "CLAUDE_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_USAGE_LOCAL_DAYS",
    "COMANDOS_USAGE_CLAUDE_MAX_FILES",
    "COMANDOS_USAGE_CODEX_MAX_FILES",
    "COMANDOS_CLAUDE_PROJECTS_DIR",
    "COMANDOS_OPENCODE_DB",
    "OPENAI_ADMIN_KEY",
    "ANTHROPIC_ADMIN_KEY",
];

pub async fn oracle(home: &TestHome) -> Option<Oracle> {
    let python = Command::new("python3")
        .args(["-c", "import sys"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !python {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    }
    // `restore_requested_webterm()` al arrancar lanzaría el `cc-webterm` real
    // del PATH y `tailscale serve`: la marca solo se crea con el oráculo vivo.
    assert!(
        !home.hooks().join("webterm-enabled").exists(),
        "webterm-enabled antes de arrancar el oráculo tocaría el terminal web real"
    );
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Igual que `xtask parity`: los ejecutables con efectos fuera del HOME temporal
    // (terminal web, tailscale, systemd, sonido, ventanas) son enlaces a `true`, y el
    // oráculo no ve el systemd ni el DBus de la sesión real.
    let fakebin = home.root.join("fakebin");
    let runtime = home.root.join("xdg-runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    fake_effects(&fakebin);
    // El `tmux` del oráculo va siempre con `-S` al socket privado de la
    // prueba: solo `TMUX_TMPDIR` no basta (tmux 3.2a cae en el servidor real
    // del usuario si ese directorio desaparece).
    // Sin tmux instalado el envoltorio llama a `true` (nunca a otro servidor).
    let real_tmux = ["/usr/bin/tmux", "/bin/tmux", "/usr/local/bin/tmux"]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or("/bin/true");
    let socket = comandos_server::dash::native::tmux::private_socket(&home.tmux_dir());
    if let Some(parent) = socket.parent() {
        use std::os::unix::fs::DirBuilderExt;
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent);
    }
    write_executable(
        &fakebin.join("tmux"),
        &tmux_guard(Path::new(real_tmux), &socket),
    );
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    for attempt in 0..ORACLE_ATTEMPTS {
        let port = oracle_port(attempt);
        let err = std::fs::File::create(home.root.join("oracle.err")).unwrap();
        let mut command = Command::new("python3");
        for key in D7_KEYS {
            command.env_remove(key);
        }
        let child = command
            .arg(repo.join("bin/cc-dash"))
            .arg(port.to_string())
            .arg("--no-open")
            .env_remove("TMUX")
            .env_remove("COMANDOS_STATE_DB")
            .env_remove("COMANDOS_USAGE_DB")
            .env_remove("COMANDOS_QUICK_TERMINAL_BASE")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("GROK_HOME")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env("HOME", &home.root)
            .env("PATH", &path)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("XDG_STATE_HOME", home.root.join(".local/state"))
            .env("TMUX_TMPDIR", home.tmux_dir())
            .env("COMANDOS_DASH_DIR", repo.join("dash"))
            // Codificación de `open()` fija: la del lado Rust (UTF-8).
            .env("LANG", "C.UTF-8")
            .env_remove("LC_ALL")
            .env_remove("LC_CTYPE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(err)
            .spawn()
            .unwrap();
        let mut oracle = Oracle {
            port,
            child,
            group: false,
        };
        match wait_ready(&mut oracle, &home.root.join("oracle.err"), "cc-dash").await {
            Start::Ready => return Some(oracle),
            Start::Retry => continue,
        }
    }
    panic!("cc-dash: {ORACLE_ATTEMPTS} puertos ocupados seguidos");
}

/// Intentos de arranque del oráculo con otro puerto si el suyo estaba tomado.
const ORACLE_ATTEMPTS: u32 = 8;

/// Puerto de un oráculo: al azar en 20000–29999, fuera del rango efímero
/// (32768–60999) del que salen `dead_port()` y los puertos del frente, así un
/// oráculo nunca ocupa el puerto «muerto» de otra prueba. Dos oráculos que
/// eligen el mismo: el segundo no puede escuchar y `wait_ready` pide otro.
fn oracle_port(attempt: u32) -> u16 {
    let mut seed = [0u8; 2];
    if getrandom::fill(&mut seed).is_err() {
        seed = (std::process::id() as u16 ^ attempt as u16).to_le_bytes();
    }
    20_000 + u16::from_le_bytes(seed) % 10_000
}

/// Si `pid` es quien escucha en `127.0.0.1:port`: un socket en LISTEN de
/// `/proc/net/tcp` con ese puerto cuyo inodo es uno de los descriptores de
/// `pid`. Sin conectar a nada: un servicio ajeno en el puerto no recibe ni
/// un byte y nunca se toma por el oráculo.
pub fn listens_on(pid: u32, port: u16) -> bool {
    let Ok(table) = std::fs::read_to_string("/proc/net/tcp") else {
        return false;
    };
    let wanted = format!(":{port:04X}");
    let inodes: Vec<String> = table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (local, state, inode) = (fields.get(1)?, fields.get(3)?, fields.get(9)?);
            (local.ends_with(&wanted) && *state == "0A").then(|| format!("socket:[{inode}]"))
        })
        .collect();
    if inodes.is_empty() {
        return false;
    }
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return false;
    };
    fds.flatten().any(|fd| {
        std::fs::read_link(fd.path())
            .is_ok_and(|target| inodes.iter().any(|i| target.as_os_str() == i.as_str()))
    })
}

pub enum Start {
    Ready,
    /// El puerto ya era de otro: reintentar con otro.
    Retry,
}

/// Espera a que el propio hijo del oráculo escuche en su puerto (no basta
/// con que algo acepte conexiones: podría ser el oráculo de otra prueba).
async fn wait_ready(oracle: &mut Oracle, err_path: &Path, what: &str) -> Start {
    let started = Instant::now();
    loop {
        if listens_on(oracle.child.id(), oracle.port) {
            return Start::Ready;
        }
        if let Ok(Some(status)) = oracle.child.try_wait() {
            let log = std::fs::read_to_string(err_path).unwrap_or_default();
            if log.contains("Address already in use") {
                return Start::Retry;
            }
            panic!("{what} salió con {status}: {log}");
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{what} no arrancó"
        );
        sleep(Duration::from_millis(50)).await;
    }
}

/// `python3 -c <guion> <repo> <args…>` sobre el HOME temporal `home`; `None`
/// sin python3. Mismo entorno que `oracle` (y que `run_python` del runtime):
/// los ejecutables con efectos fuera del HOME son enlaces a `true` (también
/// `tmux`), y el guion no ve el tmux, el systemd ni el DBus de la sesión real.
pub fn run_python(script: &str, args: &[&OsStr], home: &Path) -> Option<String> {
    let python = Command::new("python3")
        .args(["-c", "import sys"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !python {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    }
    let repo = super::repo();
    let fakebin = home.join("fakebin");
    let runtime = home.join("xdg-runtime");
    let tmux = home.join("tmux");
    for dir in [&runtime, &tmux] {
        std::fs::create_dir_all(dir).unwrap();
    }
    fake_effects(&fakebin);
    fake_tmux_true(&fakebin);
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new("python3");
    // B1: sin las claves de D7 del shell del desarrollador y con un `LANG` fijo.
    for key in D7_KEYS {
        command.env_remove(key);
    }
    let out = command
        .arg("-c")
        .arg(script)
        .arg(&repo)
        .args(args)
        .current_dir(&repo)
        .env("LANG", "C.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env("HOME", home)
        .env("PATH", &path)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("TMUX_TMPDIR", &tmux)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("COMANDOS_QUICK_TERMINAL_BASE")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8(out.stdout).unwrap())
}

// ------------------------------------------------------------ confinamiento (R1)

/// Ejecutable 0755 en `path`; sustituye lo que hubiera (también un enlace).
pub fn write_executable(path: &Path, text: &str) {
    let dir = path.parent().unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    install_executables(dir, &[(name, text.to_owned())]);
}

/// Instala ejecutables 0755 en `dir` (nombre → texto) sin que este proceso
/// abra nunca para escritura un archivo que luego se ejecuta: el texto va a
/// `<dir>/.src/` y un `sh` aparte lo copia. Si no, otro hilo de pruebas que
/// hiciera `fork` mientras el descriptor de escritura sigue abierto (hasta su
/// `exec`) provocaría `ETXTBSY` («Text file busy») al ejecutarlo.
pub fn install_executables(dir: &Path, files: &[(String, String)]) {
    let src = dir.join(".src");
    std::fs::create_dir_all(&src).unwrap();
    for (name, text) in files {
        assert!(
            !name.contains('/') && !name.is_empty(),
            "nombre inválido: {name}"
        );
        std::fs::write(src.join(name), text).unwrap();
    }
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    let sh = real_program("sh").unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let status = Command::new(sh)
        .arg("-c")
        .arg(
            "cd \"$1\" || exit 1; shift; for n in \"$@\"; do \
             rm -f \"./$n\" && cp \".src/$n\" \"./$n\" && chmod 755 \"./$n\" || exit 1; done",
        )
        .arg("sh")
        .arg(dir)
        .args(&names)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "no se pudieron instalar {names:?}");
}

/// Comillas simples de `sh`.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Un programa real por ruta absoluta (nunca un nombre que resuelva un `PATH`).
pub fn real_program(name: &str) -> Option<PathBuf> {
    ["/usr/bin", "/bin", "/usr/local/bin"]
        .into_iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|p| p.is_file())
}

/// `tmux` del `fakebin`: el Python del oráculo llama a `tmux` sin `-S`; este
/// envoltorio lo fuerza y se niega (97, sin lanzar tmux) si el directorio del
/// socket no existe.
pub fn tmux_guard(real_tmux: &Path, socket: &Path) -> String {
    let dir = socket
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    format!(
        "#!/bin/sh\n[ -d {dir} ] || {{ echo 'tmux guardián: sin socket privado' >&2; exit 97; }}\n\
         exec {real} -S {sock} \"$@\"\n",
        dir = sh_quote(&dir),
        real = sh_quote(&real_tmux.display().to_string()),
        sock = sh_quote(&socket.display().to_string()),
    )
}

/// El guardián del gemelo: como `tmux_guard`, pero además anota cada llamada en
/// `log` (formato de `fake_calls`) y ejecuta tmux con `env -i` y SOLO `env`,
/// de modo que el servidor privado y todo panel nazcan confinados aunque quien
/// llame (el frente, la prueba) traiga `DISPLAY`, el HOME o el `PATH` reales.
///
/// Una llamada con `load-buffer` guarda además su stdin, byte a byte, en
/// `tmux-stdin.log` junto a `log` (registros terminados en `\0\x1e\0`,
/// `twin::tmux_stdin`): tmux lo lee de una copia en el mismo directorio, que
/// se borra después. El `-S` y el `env -i` son los mismos en los dos caminos.
pub fn confined_tmux_guard(
    real_tmux: &Path,
    socket: &Path,
    env: &[(String, String)],
    log: &Path,
) -> String {
    let dir = socket
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let assigns: Vec<String> = env
        .iter()
        .map(|(k, v)| sh_quote(&format!("{k}={v}")))
        .collect();
    let env_bin = real_program("env").unwrap_or_else(|| PathBuf::from("/usr/bin/env"));
    let cat = real_program("cat").unwrap_or_else(|| PathBuf::from("/bin/cat"));
    let rm = real_program("rm").unwrap_or_else(|| PathBuf::from("/bin/rm"));
    let stdin_log = log.with_file_name("tmux-stdin.log");
    let stdin_copy = log.with_file_name("tmux-stdin.");
    format!(
        "#!/bin/sh\n[ -d {dir} ] || {{ echo 'tmux guardián: sin socket privado' >&2; exit 97; }}\n\
         printf '%s\\0' tmux \"$@\" \"$(printf '\\036')\" >> {log}\n\
         buffer=\n\
         for a in \"$@\"; do [ \"$a\" = load-buffer ] && buffer=1; done\n\
         if [ -n \"$buffer\" ]; then\n\
         copy={copy}$$\n\
         {cat} > \"$copy\"\n\
         {{ {cat} \"$copy\"; printf '\\000\\036\\000'; }} >> {stdin_log}\n\
         {env_bin} -i {assigns} {real} -S {sock} \"$@\" < \"$copy\"\n\
         rc=$?\n\
         {rm} -f \"$copy\"\n\
         exit $rc\n\
         fi\n\
         exec {env_bin} -i {assigns} {real} -S {sock} \"$@\"\n",
        dir = sh_quote(&dir),
        log = sh_quote(&log.display().to_string()),
        copy = sh_quote(&stdin_copy.display().to_string()),
        cat = sh_quote(&cat.display().to_string()),
        rm = sh_quote(&rm.display().to_string()),
        stdin_log = sh_quote(&stdin_log.display().to_string()),
        env_bin = sh_quote(&env_bin.display().to_string()),
        assigns = assigns.join(" "),
        real = sh_quote(&real_tmux.display().to_string()),
        sock = sh_quote(&socket.display().to_string()),
    )
}

/// Falsos que solo anotan su argv en `<HOME>/fakebin.log` y salen con 0:
/// agentes, terminales, navegadores, efectos de escritorio, red y procesos.
pub const LOGGING_FAKES: &[&str] = &[
    "claude",
    "claude-grok",
    "codex",
    "grok",
    "gemini",
    "opencode",
    "aider",
    "agy",
    "acp",
    "cc-acp",
    "claude-agent-acp",
    "codex-acp",
    "node",
    "nodejs",
    "npx",
    "npm",
    "bun",
    "bunx",
    "cc-model-proxy",
    "cc-proxy",
    "ssh-copy-id",
    "scp",
    "sftp",
    "rsync",
    "pkill",
    "killall",
    "git",
    "gh",
    "kitty",
    "tilix",
    "gnome-terminal",
    "x-terminal-emulator",
    "xterm",
    "konsole",
    "alacritty",
    "wezterm",
    "foot",
    "xfce4-terminal",
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "firefox",
    "sensible-browser",
    "wslview",
    "xdg-open",
    "gio",
    "notify-send",
    "systemctl",
    "loginctl",
    "dbus-send",
    "gdbus",
    "busctl",
    "wmctrl",
    "xdotool",
    "xclip",
    "xsel",
    "wl-copy",
    "wl-paste",
    "tailscale",
    "cc-webterm",
    "cc-webterm-attach",
    "ttyd",
    "pw-play",
    "paplay",
    "aplay",
    "spd-say",
    "piper",
];

/// Falsos que anotan y fallan: `ssh` (como el `/no-existe/ssh` del frente:
/// `ssh -O check` nunca encuentra un maestro) y `sudo`.
pub const FAILING_FAKES: &[&str] = &["ssh", "sudo"];

/// Herramientas reales inocuas (solo leen o escriben en el HOME temporal) que
/// el `fakebin` confinado enlaza: shells, utilidades de texto, `ps`/`lsof`
/// (solo leen `/proc`), `python3`, `ssh-keygen` (claves en el HOME temporal).
/// Nada de red (`curl`, `wget`, `nc`) ni de escritorio: no existen.
pub const REAL_TOOLS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "env",
    "cat",
    "sleep",
    "printf",
    "echo",
    "true",
    "false",
    "test",
    "[",
    "ls",
    "mkdir",
    "rm",
    "rmdir",
    "cp",
    "mv",
    "ln",
    "touch",
    "chmod",
    "mktemp",
    "date",
    "head",
    "tail",
    "sed",
    "grep",
    "awk",
    "gawk",
    "tr",
    "cut",
    "sort",
    "uniq",
    "wc",
    "seq",
    "expr",
    "stat",
    "readlink",
    "realpath",
    "dirname",
    "basename",
    "tee",
    "xargs",
    "find",
    "id",
    "whoami",
    "uname",
    "hostname",
    "nproc",
    "ps",
    "lsof",
    "stty",
    "tput",
    "clear",
    "which",
    "python3",
    "fc-list",
    "qrencode",
    "ssh-keygen",
];

/// Una llamada a un falso (o al guardián de tmux): nombre y argumentos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeCall {
    pub name: String,
    pub args: Vec<String>,
}

/// Llamadas anotadas en `<root>/fakebin.log`, en orden.
pub fn fake_calls(root: &Path) -> Vec<FakeCall> {
    calls_in(&root.join("fakebin.log"))
}

/// Registro `printf '%s\0' <nombre> "$@" $'\036'`: cada llamada termina en
/// `\0\x1e\0` (un argumento nunca contiene `\0`).
pub fn calls_in(log: &Path) -> Vec<FakeCall> {
    let bytes = std::fs::read(log).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    text.split("\u{0}\u{1e}\u{0}")
        .filter(|record| !record.is_empty())
        .map(|record| {
            let mut parts = record.split('\u{0}').map(str::to_owned);
            FakeCall {
                name: parts.next().unwrap_or_default(),
                args: parts.collect(),
            }
        })
        .collect()
}

fn logging_fake(root: &Path, name: &str, code: i32) -> String {
    format!(
        "#!/bin/sh\nprintf '%s\\0' {name} \"$@\" \"$(printf '\\036')\" >> {log}\nexit {code}\n",
        name = sh_quote(name),
        log = sh_quote(&root.join("fakebin.log").display().to_string()),
    )
}

/// `systemd-run` del gemelo (P17): anota su argv como `fake_scope`
/// (`fake_scope_calls`), y SOLO ejecuta una cola de tmux (`tmux` o el guardián)
/// con las banderas de `scope_cmd` (`--user --scope --collect --quiet`; si no,
/// 97). Cualquier otra cola (las terminales de `spawn_terminal`) se anota y no
/// corre.
fn confined_scope(root: &Path, guard: &Path) -> String {
    let dir = root.join("fakescope");
    let log = dir.join("argv");
    format!(
        "#!/bin/sh\nmkdir -p {dir}\n\
         printf '%s\\0' systemd-run \"$@\" \"$(printf '\\036')\" >> {log}\n\
         flags=\n\
         while [ $# -gt 0 ]; do case \"$1\" in --*) flags=\"$flags $1\"; shift;; *) break;; esac; done\n\
         [ $# -gt 0 ] || exit 0\n\
         case \"$1\" in tmux|{guard}) ;; *) exit 0;; esac\n\
         [ \"$flags\" = ' --user --scope --collect --quiet' ] || \
         {{ echo 'systemd-run falso: faltan las banderas de scope_cmd' >&2; exit 97; }}\n\
         shift\nexec {guard} \"$@\"\n",
        dir = sh_quote(&dir.display().to_string()),
        log = sh_quote(&log.display().to_string()),
        guard = sh_quote(&guard.display().to_string()),
    )
}

/// Prepara el confinamiento de `home` (R1 del pre-flight 2f): `fakebin` con
/// marca `.confined` (lo lee `TestHome::confined_env`: `PATH` = solo él), las
/// herramientas inocuas enlazadas, los falsos, el guardián de tmux con entorno
/// limpio, el `systemd-run` que solo ejecuta tmux, y `cc-notify.conf` con
/// `DESKTOP_NOTIFY=0` si la prueba no sembró otro. `extra` (nombre → texto)
/// añade o sustituye ejecutables, nunca el guardián ni el `systemd-run`.
pub fn confined_fakebin(home: &TestHome, extra: &[(String, String)]) -> PathBuf {
    let fakebin = home.root.join("fakebin");
    std::fs::create_dir_all(&fakebin).unwrap();
    std::fs::write(fakebin.join(".confined"), "").unwrap();
    // Antes de leer `confined_env` (el guardián lo copia): con el archivo en su
    // sitio, el entorno lleva `PYTHONPATH` a él.
    let site = home.root.join(SITE_DIR);
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("sitecustomize.py"), SITECUSTOMIZE).unwrap();
    for name in REAL_TOOLS {
        if let Some(real) = real_program(name) {
            let link = fakebin.join(name);
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(real, &link).unwrap();
        }
    }
    let mut files: Vec<(String, String)> = LOGGING_FAKES
        .iter()
        .map(|name| ((*name).to_owned(), logging_fake(&home.root, name, 0)))
        .chain(
            FAILING_FAKES
                .iter()
                .map(|name| ((*name).to_owned(), logging_fake(&home.root, name, 1))),
        )
        .collect();
    let socket = comandos_server::dash::native::tmux::private_socket(&home.tmux_dir());
    if let Some(parent) = socket.parent() {
        use std::os::unix::fs::DirBuilderExt;
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent);
    }
    let guard = fakebin.join("tmux");
    let real_tmux = real_program("tmux").unwrap_or_else(|| PathBuf::from("/no-existe/tmux"));
    files.push((
        "tmux".into(),
        confined_tmux_guard(
            &real_tmux,
            &socket,
            &home.confined_env(),
            &home.root.join("tmux.log"),
        ),
    ));
    files.push(("systemd-run".into(), confined_scope(&home.root, &guard)));
    for (name, text) in extra {
        assert!(
            !matches!(name.as_str(), "tmux" | "systemd-run") && !name.contains('/'),
            "fakebin_extra no puede sustituir {name}: es parte del confinamiento"
        );
        files.push((name.clone(), text.clone()));
    }
    install_executables(&fakebin, &files);
    let conf = home.hooks().join("cc-notify.conf");
    if !conf.exists() {
        std::fs::write(&conf, "DESKTOP_NOTIFY=0\n").unwrap();
    }
    fakebin
}

/// Directorio (relativo al HOME de la prueba) del `sitecustomize.py` del
/// confinamiento; `TestHome::confined_env` pone `PYTHONPATH` en él si existe.
pub const SITE_DIR: &str = "pysite";

/// `sitecustomize.py` generado en cada HOME confinado (no es Python del
/// repositorio): todo Python que nazca con el entorno confinado (oráculo,
/// `run_dash`, guiones de los paneles, `cc-acp`) lo importa al arrancar. Deja
/// los respaldos `_USER_BIN_DIRS` de `lib/providers.py` y `lib/acp.py` solo con
/// directorios del HOME (`~/…`): sin él, un nombre que no esté en el `fakebin`
/// caería en el `/usr/local/bin` real. Es el mismo recorte que el frente del
/// gemelo hace con `NativeOptions::user_bin_dirs`.
pub const SITECUSTOMIZE: &str = r#"
import sys, importlib.abc, importlib.machinery

class _TwinUserBinDirs(importlib.abc.MetaPathFinder):
    def find_spec(self, name, path, target=None):
        if name not in ("providers", "acp"):
            return None
        spec = importlib.machinery.PathFinder.find_spec(name, path)
        if spec is None or spec.loader is None:
            return spec
        original = spec.loader.exec_module
        def exec_module(module):
            original(module)
            dirs = getattr(module, "_USER_BIN_DIRS", None)
            if isinstance(dirs, tuple):
                module._USER_BIN_DIRS = tuple(d for d in dirs if d.startswith("~"))
        spec.loader.exec_module = exec_module
        return spec

sys.meta_path.insert(0, _TwinUserBinDirs())
"#;

/// Opciones del oráculo confinado (`oracle_with`, `run_dash_with`).
#[derive(Default, Clone)]
pub struct OracleOpts {
    /// Ejecutables extra del `fakebin` (nombre → texto).
    pub fakebin_extra: Vec<(String, String)>,
    /// Python que corre tras cargar `cc-dash` (módulo `dash`) y antes de `main()`.
    pub python_prelude: String,
    /// Puertos de `127.0.0.1` a los que el Python sí puede conectar (servidores
    /// de la propia prueba). Todo lo demás —4777–4782, DNS, otros hosts,
    /// sockets Unix fuera del HOME— se rechaza.
    pub allow_ports: Vec<u16>,
    /// Bucles de fondo de `main()` que siguen vivos (por omisión, ninguno).
    pub keep_loops: Vec<String>,
    /// Variables extra del proceso (después del entorno confinado).
    pub extra_env: Vec<(String, String)>,
}

/// Prólogo de todo Python confinado: cierra la red al nivel de `socket`
/// (cubre `urllib`, `http.client` y `requests`), carga `bin/cc-dash` como el
/// módulo `dash` (sin ejecutar `main`) y sustituye sus bucles de fondo por una
/// función vacía salvo los de `COMANDOS_TWIN_KEEP_LOOPS`. Deja la marca
/// `~/.comandos-twin-guard` para que las pruebas comprueben que corrió.
const CONFINED_PRELUDE: &str = r#"
import os, sys, socket, importlib.machinery, importlib.util
_TWIN_HOME = os.environ["HOME"]
_TWIN_PORTS = {int(p) for p in os.environ.get("COMANDOS_TWIN_ALLOW_PORTS", "").split(",") if p}
_TWIN_LOCAL = ("127.0.0.1", "localhost", "::1")

def _twin_check(family, addr):
    if family == socket.AF_UNIX:
        path = addr.decode() if isinstance(addr, bytes) else str(addr)
        if path.startswith(_TWIN_HOME + os.sep):
            return
        raise ConnectionRefusedError(111, "gemelo: socket Unix fuera del HOME de la prueba: %s" % path)
    if isinstance(addr, tuple) and len(addr) >= 2 and addr[0] in _TWIN_LOCAL and addr[1] in _TWIN_PORTS:
        return
    raise ConnectionRefusedError(111, "gemelo: red cerrada (%r)" % (addr,))

_twin_connect = socket.socket.connect
_twin_connect_ex = socket.socket.connect_ex
_twin_sendto = socket.socket.sendto

def _twin_c(self, addr):
    _twin_check(self.family, addr)
    return _twin_connect(self, addr)

def _twin_cx(self, addr):
    try:
        _twin_check(self.family, addr)
    except ConnectionRefusedError:
        return 111
    return _twin_connect_ex(self, addr)

def _twin_st(self, data, *rest):
    _twin_check(self.family, rest[-1])
    return _twin_sendto(self, data, *rest)

socket.socket.connect = _twin_c
socket.socket.connect_ex = _twin_cx
socket.socket.sendto = _twin_st
_twin_gai = socket.getaddrinfo

def _twin_getaddrinfo(host, *args, **kwargs):
    name = host.decode() if isinstance(host, bytes) else host
    if name not in (None,) + _TWIN_LOCAL:
        raise socket.gaierror(socket.EAI_NONAME, "gemelo: sin DNS")
    return _twin_gai(host, *args, **kwargs)

socket.getaddrinfo = _twin_getaddrinfo
_twin_path = sys.argv[1]
sys.argv = [_twin_path] + sys.argv[2:]
# Como `python3 bin/cc-dash`: el directorio del guion va primero en `sys.path`.
sys.path.insert(0, os.path.dirname(os.path.realpath(_twin_path)))
_twin_loader = importlib.machinery.SourceFileLoader("cc_dash", _twin_path)
dash = importlib.util.module_from_spec(importlib.util.spec_from_loader("cc_dash", _twin_loader))
sys.modules["cc_dash"] = dash
_twin_loader.exec_module(dash)
_twin_keep = set(filter(None, os.environ.get("COMANDOS_TWIN_KEEP_LOOPS", "").split(",")))

def _twin_loop_off(*args, **kwargs):
    return None

_twin_off = []
for _twin_name in ("_model_watch_loop", "_news_editions_loop", "_notices_push_loop", "_limits_snapshot_loop"):
    if _twin_name not in _twin_keep:
        setattr(dash, _twin_name, _twin_loop_off)
        _twin_off.append(_twin_name)
with open(os.path.join(_TWIN_HOME, ".comandos-twin-guard"), "w") as _twin_f:
    _twin_f.write("red cerrada; bucles apagados: %s\n" % ",".join(_twin_off))
"#;

/// `python3 -c <prólogo + script> <repo>/bin/cc-dash <args…>` con el entorno
/// confinado de `home`: `env_clear` + `TestHome::confined_env` + variables del
/// prólogo. Nunca hereda nada del proceso de pruebas.
fn confined_python(home: &TestHome, script: &str, args: &[String], opts: &OracleOpts) -> Command {
    confined_fakebin(home, &opts.fakebin_extra);
    let repo = std::fs::canonicalize(super::repo()).unwrap();
    let python = real_program("python3").unwrap_or_else(|| PathBuf::from("/usr/bin/python3"));
    let mut command = Command::new(python);
    command
        .arg("-c")
        .arg(format!("{CONFINED_PRELUDE}\n{script}\n"))
        .arg(repo.join("bin/cc-dash"))
        .args(args)
        .current_dir(&home.root)
        .env_clear()
        .envs(home.confined_env())
        .env("COMANDOS_DASH_DIR", repo.join("dash"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env(
            "COMANDOS_TWIN_ALLOW_PORTS",
            opts.allow_ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )
        .env("COMANDOS_TWIN_KEEP_LOOPS", opts.keep_loops.join(","))
        .envs(opts.extra_env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null());
    command
}

fn python_available() -> bool {
    let ok = real_program("python3").is_some_and(|python| {
        Command::new(python)
            .args(["-c", "import sys"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    if !ok {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
    }
    ok
}

/// `cc-dash` confinado (R1): mismo HOME que la prueba, `PATH` = solo el
/// `fakebin` confinado, sin bucles de fondo, red cerrada salvo
/// `opts.allow_ports`. `oracle()` sigue siendo el de 2b–2e.
pub async fn oracle_with(home: &TestHome, opts: OracleOpts) -> Option<Oracle> {
    if !python_available() {
        return None;
    }
    assert!(
        !home.hooks().join("webterm-enabled").exists(),
        "webterm-enabled antes de arrancar el oráculo tocaría el terminal web"
    );
    let script = format!("{}\ndash.main()\n", opts.python_prelude);
    let err_path = home.root.join("oracle.err");
    for attempt in 0..ORACLE_ATTEMPTS {
        let port = oracle_port(attempt);
        let err = std::fs::File::create(&err_path).unwrap();
        let child = confined_python(
            home,
            &script,
            &[port.to_string(), "--no-open".into()],
            &opts,
        )
        .stdout(Stdio::null())
        .stderr(err)
        .process_group(0)
        .spawn()
        .unwrap();
        let mut oracle = Oracle {
            port,
            child,
            group: true,
        };
        // `oracle` ya es dueño del hijo: si el arranque falla, su `Drop` mata el grupo.
        match wait_ready(&mut oracle, &err_path, "cc-dash confinado").await {
            Start::Ready => return Some(oracle),
            Start::Retry => continue,
        }
    }
    panic!("cc-dash confinado: {ORACLE_ATTEMPTS} puertos ocupados seguidos");
}

/// Código Python con `dash` = `bin/cc-dash` cargado, en el entorno confinado de
/// `home` (P21: `python_dash` de 2f-1 vive aquí). `None` sin python3; la
/// prueba falla si el código falla. Devuelve la salida estándar.
pub fn run_dash(home: &TestHome, code: &str) -> Option<String> {
    run_dash_with(home, code, &OracleOpts::default())
}

pub fn run_dash_with(home: &TestHome, code: &str, opts: &OracleOpts) -> Option<String> {
    if !python_available() {
        return None;
    }
    let out = confined_python(home, code, &[], opts).output().unwrap();
    assert!(
        out.status.success(),
        "python confinado: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8(out.stdout).unwrap())
}
