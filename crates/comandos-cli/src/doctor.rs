//! Diagnostics without GTK initialization or state migrations. Fixes are typed,
//! confirmed actions; no shell interprets configuration values.
use comandos_store::domains::{DomainStore, catalog};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, BufRead, Read, Write},
    os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output},
};

// The oracle's sed 2..10 includes this final shell-options line in help output.
const HELP: &str = "cc-doctor — diagnóstico de ComandOS. Chequea plataforma, deps, servicios y\nfixes con confirmación. Reemplaza al .wslg-check.sh artesanal.\n\nUso:\n  cc-doctor            chequea, imprime tabla; exit != 0 si algo FAIL\n  cc-doctor --fix      ofrece arreglar cada FAIL con confirmación\n  cc-doctor --json     una línea JSON por check\n  cc-doctor <sección>  solo esa sección: core | desktop | audio | remote\nset -uo pipefail\n";
const VOICE: &str = "es_MX-ald-medium";
const VOICE_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main/es/es_MX/ald/medium/es_MX-ald-medium.onnx";
const VOICE_SHA: &str = "019b3803293c93e34a206dd2e53a3889209a514e786fd7144f7b70196c579b63";

#[derive(Clone)]
enum Fix {
    Apt(Vec<&'static str>),
    Exec(&'static str, Vec<String>),
    Voice(String),
}
impl Fix {
    fn hint(&self) -> String {
        match self {
            Self::Apt(packages) => format!("apt_install_confirmed {}", packages.join(" ")),
            Self::Exec(program, args) => format!("{program} {}", args.join(" ")),
            Self::Voice(voice) => format!("_fix_piper_voice {voice}"),
        }
    }
}
struct Check {
    section: &'static str,
    name: String,
    status: &'static str,
    detail: String,
    fix: Option<Fix>,
}
struct Doctor {
    home: PathBuf,
    platform: &'static str,
    codename: String,
    rows: Vec<Check>,
}
fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
fn points_to_bin(alias: &Path, bin: &Path) -> bool {
    let Ok(link) = fs::read_link(alias) else {
        return false;
    };
    let target = if link.is_absolute() {
        link
    } else {
        alias.parent().unwrap_or(Path::new("")).join(link)
    };
    // Resolve directories, not the final bin symlink: linking to the frozen
    // release directly would silently miss the next install/rollback swap.
    target.file_name() == bin.file_name()
        && target
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .is_some_and(|parent| Some(parent) == bin.parent().and_then(|p| p.canonicalize().ok()))
        && executable(alias)
}
fn exec_starts(body: &str) -> Vec<String> {
    let mut commands = vec![];
    let mut service = false;
    let mut logical = String::new();
    for line in body.lines() {
        let line = line.trim_start();
        if line.starts_with(['#', ';']) {
            continue;
        }
        logical.push_str(line);
        if logical.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1 {
            logical.pop();
            logical.push(' ');
            continue;
        }
        let line = logical.trim();
        if line.starts_with('[') && line.ends_with(']') {
            service = line == "[Service]";
        } else if service
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "ExecStart"
        {
            if value.trim().is_empty() {
                commands.clear();
            } else {
                commands.push(value.trim().to_owned());
            }
        }
        logical.clear();
    }
    commands
}
fn which(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(name))
        .find(|path| executable(path))
}
fn output(name: &str, args: &[&str]) -> Option<Output> {
    Command::new(which(name)?).args(args).output().ok()
}
fn text(name: &str, args: &[&str]) -> String {
    output(name, args)
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_owned()
        })
        .unwrap_or_default()
}
fn succeeds(name: &str, args: &[&str]) -> bool {
    output(name, args).is_some_and(|o| o.status.success())
}
fn execute(name: &str, args: &[&str], out: &mut String) -> bool {
    match output(name, args) {
        Some(o) => {
            out.push_str(&String::from_utf8_lossy(&o.stdout));
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            o.status.success()
        }
        None => {
            eprintln!("cc-doctor: no se pudo ejecutar {name}");
            false
        }
    }
}
fn setting(bytes: &str, key: &str, apostrophes: bool) -> String {
    bytes
        .lines()
        .find_map(|line| {
            let mut fields = line.split('=');
            (fields.next()? == key).then(|| {
                fields
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .filter(|c| *c != '"' && (!apostrophes || *c != '\''))
                    .collect()
            })
        })
        .unwrap_or_default()
}
fn read_setting(path: &Path, key: &str, apostrophes: bool) -> String {
    fs::read_to_string(path)
        .map(|s| setting(&s, key, apostrophes))
        .unwrap_or_default()
}
fn lint_entries(root: &Path, errors: &mut Vec<String>) -> Vec<fs::DirEntry> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return vec![],
        Err(e) => {
            errors.push(format!("{}: {e}", root.display()));
            return vec![];
        }
    };
    entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(e) => {
                errors.push(format!("{}: no se pudo leer entrada: {e}", root.display()));
                None
            }
        })
        .collect()
}
impl Doctor {
    fn new(home: PathBuf) -> Self {
        let sys = env::var("CC_MOCK_UNAME")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| text("uname", &["-s"]));
        let os = PathBuf::from(
            env::var_os("CC_MOCK_OS_RELEASE_FILE").unwrap_or_else(|| "/etc/os-release".into()),
        );
        let kernel = PathBuf::from(
            env::var_os("CC_MOCK_OSRELEASE_FILE")
                .unwrap_or_else(|| "/proc/sys/kernel/osrelease".into()),
        );
        let platform = if sys == "Darwin" {
            "darwin"
        } else if sys != "Linux" {
            "linux-other"
        } else if fs::read_to_string(kernel)
            .is_ok_and(|s| s.to_ascii_lowercase().contains("microsoft"))
        {
            if read_setting(&os, "ID", false) == "ubuntu" {
                "linux-wsl-ubuntu"
            } else {
                "linux-other"
            }
        } else {
            "linux-native"
        };
        Self {
            home,
            platform,
            codename: read_setting(&os, "VERSION_CODENAME", false),
            rows: vec![],
        }
    }
    fn check(
        &mut self,
        section: &'static str,
        name: impl Into<String>,
        status: &'static str,
        detail: impl Into<String>,
        fix: Option<Fix>,
    ) {
        self.rows.push(Check {
            section,
            name: name.into(),
            status,
            detail: detail.into(),
            fix,
        });
    }
    fn command_check(
        &mut self,
        section: &'static str,
        name: &'static str,
        args: &[&str],
        packages: Vec<&'static str>,
    ) {
        if which(name).is_some() {
            self.check(section, name, "pass", text(name, args), None)
        } else {
            self.check(
                section,
                name,
                "fail",
                "no instalado",
                Some(Fix::Apt(packages)),
            )
        }
    }
    fn core(&mut self) {
        let sec = "Núcleo";
        self.command_check(sec, "tmux", &["-V"], vec!["tmux"]);
        self.command_check(sec, "jq", &["--version"], vec!["jq"]);
        if which("grok").is_some() {
            self.check(
                sec,
                "Grok Build",
                "pass",
                text("grok", &["--version"])
                    .lines()
                    .next()
                    .unwrap_or_default(),
                None,
            );
            let auth = fs::metadata(self.home.join(".grok/auth.json")).is_ok_and(|m| m.len() > 0);
            self.check(
                sec,
                "Grok auth nativa",
                if auth { "pass" } else { "warn" },
                if auth {
                    "suscripción configurada (sin exponer tokens)"
                } else {
                    "sin login"
                },
                (!auth).then(|| Fix::Exec("grok", vec!["login".into()])),
            );
            let hooks = fs::read(self.home.join(".grok/hooks/comandos.json")).is_ok_and(|s| {
                s.windows(b"StopCancelled".len())
                    .any(|w| w == b"StopCancelled")
            });
            self.check(
                sec,
                "Grok hooks",
                if hooks { "pass" } else { "warn" },
                if hooks {
                    "pane/status/error conectados"
                } else {
                    "faltan eventos de error/cancelación"
                },
                (!hooks).then(|| Fix::Exec("comandos", vec!["agents".into(), "setup".into()])),
            );
        } else {
            self.check(sec, "Grok Build", "warn", "no instalado", None)
        }
        let systemd = env::var("CC_MOCK_SYSTEMD_STATE")
            .unwrap_or_else(|_| text("systemctl", &["--user", "is-system-running"]));
        let ok = matches!(systemd.as_str(), "running" | "degraded");
        self.check(
            sec,
            "systemd --user",
            if ok { "pass" } else { "fail" },
            if ok { "running" } else { "no responde" },
            None,
        );
        let dash = succeeds("curl", &["-s", "-m", "2", "http://127.0.0.1:4777/prefs"]);
        self.check(
            sec,
            "cc-dash",
            if dash { "pass" } else { "fail" },
            if dash {
                "responde en 127.0.0.1:4777"
            } else {
                "no responde"
            },
            (!dash).then(|| {
                Fix::Exec(
                    "systemctl",
                    vec!["--user".into(), "start".into(), "cc-dash".into()],
                )
            }),
        );
        let notify = text("ss", &["-ltn"]).contains(":4778 ");
        self.check(
            sec,
            "cc-notifyd",
            if notify { "pass" } else { "warn" },
            if notify {
                "escucha en 127.0.0.1:4778"
            } else {
                "no está escuchando"
            },
            (!notify).then(|| {
                Fix::Exec(
                    "systemctl",
                    vec!["--user".into(), "start".into(), "cc-notifyd".into()],
                )
            }),
        );
        let hooks = executable(&self.home.join(".claude/hooks/cc-notify.sh"));
        self.check(
            sec,
            "hooks",
            if hooks { "pass" } else { "fail" },
            if hooks {
                "instalados en ~/.claude/hooks"
            } else {
                "no instalados"
            },
            (!hooks).then(|| {
                Fix::Exec(
                    "comandos",
                    vec!["install".into(), "--link".into(), "cc-notify.sh".into()],
                )
            }),
        );
        self.native_state();
    }
    fn native_state(&mut self) {
        let sec = "Estado Rust";
        let staged = self.home.join(".local/share/comandos/bin/comandos");
        let releases = crate::install::release::list_releases(&self.home);
        let mut active = false;
        match releases {
            Ok(releases) => {
                for release in releases {
                    let dir = release.path.parent().unwrap_or(Path::new(""));
                    let protocol = crate::install::manifest::release_protocol(dir);
                    let valid = dir.symlink_metadata().is_ok_and(|m| m.is_dir())
                        && release.path.symlink_metadata().is_ok_and(|m| m.is_file())
                        && executable(&release.path)
                        && protocol >= crate::install::manifest::STATE_PROTOCOL;
                    let current = release.current
                        && fs::canonicalize(&staged).ok() == fs::canonicalize(&release.path).ok();
                    active |= current && valid;
                    self.check(
                        sec,
                        format!("release {}", release.id),
                        if valid { "pass" } else { "fail" },
                        format!(
                            "protocolo {protocol}{}",
                            if current { " · actual" } else { "" }
                        ),
                        None,
                    );
                }
            }
            Err(e) => self.check(sec, "releases", "fail", e, None),
        }
        self.check(
            sec,
            "release activa",
            if active { "pass" } else { "fail" },
            if active {
                "bin/comandos apunta a una release compatible"
            } else {
                "ausente, incompleta o incompatible"
            },
            (!active).then(|| Fix::Exec("comandos", vec!["install".into(), "--stage".into()])),
        );
        for name in crate::dispatch::alias_names().filter(|name| name.starts_with("cc-")) {
            let path = if matches!(name, "cc-notify.sh" | "cc-usage-tool.sh" | "cc-status.sh") {
                self.home.join(".claude/hooks").join(name)
            } else {
                self.home.join(".local/bin").join(name)
            };
            let exists = path.symlink_metadata().is_ok();
            let valid = points_to_bin(&path, &staged);
            self.check(
                sec,
                format!("alias {name}"),
                if valid {
                    "pass"
                } else if exists {
                    "fail"
                } else {
                    "warn"
                },
                if valid {
                    "apunta a bin/comandos"
                } else if exists {
                    "no apunta a bin/comandos"
                } else {
                    "no instalado"
                },
                Some(Fix::Exec(
                    "comandos",
                    vec!["install".into(), "--link".into(), name.into()],
                )),
            );
        }
        self.units();
        match (DomainStore { home: &self.home }).modes_readonly() {
            Ok(modes) => {
                for (name, mode) in modes {
                    self.check(
                        sec,
                        format!("dominio {name}"),
                        "pass",
                        format!("{mode:?}").to_lowercase(),
                        None,
                    )
                }
            }
            Err(e) => self.check(sec, "modos de dominio", "fail", e.to_string(), None),
        }
        self.home_lint();
    }
    fn units(&mut self) {
        let dir = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(".config"))
            .join("systemd/user");
        let mut units = match fs::read_dir(&dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|s| s == "service"))
                .map(|e| e.path())
                .collect::<Vec<_>>(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => vec![],
            Err(e) => {
                self.check(
                    "Estado Rust",
                    "unidades systemd",
                    "fail",
                    e.to_string(),
                    None,
                );
                return;
            }
        };
        units.sort();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new(""));
        for path in units {
            let name = format!(
                "unidad {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            match fs::read_to_string(&path) {
                Ok(body) => {
                    let references = exec_starts(&body).iter().any(|s| {
                        s.contains(&repo.to_string_lossy().to_string()) || s.contains("/ComandOS/")
                    });
                    self.check(
                        "Estado Rust",
                        name,
                        if references { "fail" } else { "pass" },
                        if references {
                            "ExecStart depende del checkout"
                        } else {
                            "ExecStart independiente del checkout"
                        },
                        None,
                    );
                }
                Err(e) => self.check("Estado Rust", name, "fail", e.to_string(), None),
            }
        }
    }
    fn home_lint(&mut self) {
        let roots = [
            ("H", self.home.join(".claude/hooks")),
            ("STATE", self.home.join(".local/state/comandos")),
            ("SHARE", self.home.join(".local/share/comandos")),
        ];
        let mut leftovers = vec![];
        let mut errors = vec![];
        let mut pending: Vec<_> = roots.into_iter().collect();
        while let Some((symbol, root)) = pending.pop() {
            for entry in lint_entries(&root, &mut errors) {
                let symbolic = format!("{symbol}/{}", entry.file_name().to_string_lossy());
                let classification = catalog::file_classification(&symbolic);
                if classification == "resto" {
                    leftovers.push(entry.path());
                    continue;
                }
                let kind = match entry.file_type() {
                    Ok(kind) => kind,
                    Err(e) => {
                        errors.push(format!(
                            "{}: no se pudo inspeccionar entrada: {e}",
                            entry.path().display()
                        ));
                        continue;
                    }
                };
                // Catalog directory prefixes include a slash. Classify the root
                // itself by lstat metadata, including broken symlinks, without
                // traversing their targets or loading legacy assets.
                if (kind.is_dir() || kind.is_symlink())
                    && catalog::file_classification(&format!("{symbolic}/")) == "resto"
                {
                    leftovers.push(entry.path());
                    continue;
                }
                // Do not traverse release/web/cache trees, symlinks, or D3 archives.
                if classification == "sin-dominio" && symbolic != "SHARE/backups" && kind.is_dir() {
                    // Known active subdirectories only. Status documents are metadata-only.
                    if [
                        "H/state",
                        "STATE/extensions",
                        "SHARE/extensions-venv",
                        "SHARE/extensions",
                        "SHARE/codex-yolo",
                    ]
                    .contains(&symbolic.as_str())
                    {
                        if symbolic.starts_with("SHARE/") {
                            leftovers.push(entry.path())
                        } else {
                            for child in lint_entries(&entry.path(), &mut errors) {
                                let child_symbol =
                                    format!("{symbolic}/{}", child.file_name().to_string_lossy());
                                if catalog::file_classification(&child_symbol) == "resto" {
                                    leftovers.push(child.path());
                                }
                            }
                        }
                    }
                }
            }
        }
        leftovers.sort();
        errors.sort();
        let mut details = leftovers
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>();
        details.extend(
            errors
                .into_iter()
                .map(|error| format!("lectura incompleta: {error}")),
        );
        self.check(
            "Estado Rust",
            "restos D9",
            if details.is_empty() { "pass" } else { "warn" },
            if details.is_empty() {
                "sin restos conocidos".into()
            } else {
                details.join(" · ")
            },
            None,
        );
    }
    fn library(&mut self, name: &'static str, package: &str, soname: &str, apt: &'static str) {
        let mut dirs = env::var_os("LD_LIBRARY_PATH")
            .map(|p| env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default();
        let pkg = text("pkg-config", &["--variable=libdir", package]);
        if !pkg.is_empty() {
            dirs.push(PathBuf::from(pkg));
        }
        let cache = text("ldconfig", &["-p"]);
        let cached = cache.lines().find_map(|line| {
            let (left, right) = line.split_once("=>")?;
            (left.split_whitespace().next() == Some(soname)).then(|| PathBuf::from(right.trim()))
        });
        let found = dirs
            .into_iter()
            .map(|d| d.join(soname))
            .chain(cached)
            .find(|p| p.is_file());
        self.check(
            "App de escritorio (cc-app)",
            name,
            if found.is_some() { "pass" } else { "fail" },
            found
                .map(|p| format!("biblioteca disponible: {}", p.display()))
                .unwrap_or_else(|| format!("{soname} no disponible")),
            Some(Fix::Apt(vec![apt])),
        );
    }
    fn desktop(&mut self) {
        let sec = "App de escritorio (cc-app)";
        if self.platform == "darwin" {
            self.check(sec, "cc-app GTK", "skip", "no aplica en macOS", None);
            return;
        }
        // Actual comandos-app ABI: gtk 0.18 / GTK3, WebKit2GTK 4.1.
        // comandos-term owns its terminal; VTE/GTK4/Python GI are not dependencies.
        self.library("GTK 3", "gtk+-3.0", "libgtk-3.so.0", "libgtk-3-0");
        self.library(
            "WebKit2GTK 4.1",
            "webkit2gtk-4.1",
            "libwebkit2gtk-4.1.so.0",
            "libwebkit2gtk-4.1-0",
        );
        if self.platform == "linux-wsl-ubuntu" {
            let display = env::var("DISPLAY").unwrap_or_default();
            let wayland = env::var("WAYLAND_DISPLAY").unwrap_or_default();
            let ok = !display.is_empty() && !wayland.is_empty();
            self.check(
                sec,
                "WSLg display",
                if ok { "pass" } else { "fail" },
                if ok {
                    format!("DISPLAY={display} · WAYLAND_DISPLAY={wayland}")
                } else {
                    "$DISPLAY o $WAYLAND_DISPLAY vacíos".into()
                },
                None,
            );
            let installed = which("wslview").is_some();
            self.check(
                sec,
                "wslview",
                if installed { "pass" } else { "fail" },
                if installed { "installed" } else { "missing" },
                Some(Fix::Apt(vec!["wslu"])),
            );
        }
        let installed = which("wmctrl").is_some();
        self.check(
            sec,
            "wmctrl",
            if installed { "pass" } else { "warn" },
            if installed { "installed" } else { "missing" },
            Some(Fix::Apt(vec!["wmctrl"])),
        );
        let font = text("fc-list", &[])
            .to_ascii_lowercase()
            .contains("jetbrainsmono nerd font mono");
        if which("fc-list").is_some() {
            self.check(
                sec,
                "JetBrainsMono NF Mono",
                if font { "pass" } else { "warn" },
                if font {
                    "cacheada por fontconfig"
                } else {
                    "no cacheada — usará el fallback (Monospace del sistema)"
                },
                None,
            );
        }
        let staged = self.home.join(".local/share/comandos/bin/comandos");
        let web = fs::canonicalize(staged)
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("web")));
        let valid = web.as_ref().is_some_and(|p| {
            let manifest = p.join("manifest.json");
            manifest.symlink_metadata().is_ok_and(|m| m.is_file())
                && fs::read(manifest)
                    .ok()
                    .and_then(|b| {
                        serde_json::from_slice::<comandos_core::web_assets::Manifest>(&b).ok()
                    })
                    .is_some_and(|m| {
                        m.check_paths().is_ok()
                            && !m.files.is_empty()
                            && m.files.values().all(|rel| {
                                p.join(rel).symlink_metadata().is_ok_and(|s| s.is_file())
                            })
                    })
        });
        self.check(
            sec,
            "web nativa",
            "fail",
            "manifiesto ausente o incompleto",
            None,
        );
        if valid && let Some(row) = self.rows.last_mut() {
            row.status = "pass";
            row.detail = "manifiesto y archivos presentes".into();
        }
        if self.platform == "linux-wsl-ubuntu" && which("powershell.exe").is_some() {
            let appdata = text(
                "powershell.exe",
                &["-NoProfile", "-Command", "[Console]::Write($env:APPDATA)"],
            )
            .replace('\r', "");
            if !appdata.is_empty() {
                let distro = env::var("WSL_DISTRO_NAME").unwrap_or_else(|_| "Ubuntu".into());
                let path = PathBuf::from(text("wslpath", &[&appdata])).join(format!(
                    "Microsoft/Windows/Start Menu/Programs/{distro}/ComandOS ({distro}).lnk"
                ));
                let present = path.is_file();
                self.check(
                    sec,
                    "Start Menu (Windows)",
                    if present { "pass" } else { "warn" },
                    if present {
                        "ComandOS aparece en Inicio"
                    } else {
                        "shortcut ausente"
                    },
                    Some(Fix::Exec("cc-winstart", vec![])),
                );
            }
        }
    }
    fn audio(&mut self) {
        let sec = "Audio y voz";
        if self.platform == "linux-wsl-ubuntu" {
            let pulse = env::var_os("CC_MOCK_PULSE_SERVER")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/mnt/wslg/PulseServer".into());
            let alive = fs::metadata(pulse).is_ok_and(|m| m.file_type().is_socket());
            self.check(
                sec,
                "WSLg PulseServer",
                if alive { "pass" } else { "warn" },
                if alive {
                    "/mnt/wslg/PulseServer alive"
                } else {
                    "/mnt/wslg/PulseServer missing — audio no funcionará"
                },
                None,
            );
        }
        if which("pw-play").is_some() {
            self.check(sec, "pw-play", "pass", "installed", None)
        } else if which("paplay").is_some() {
            self.check(sec, "paplay", "pass", "installed", None)
        } else {
            self.check(
                sec,
                "paplay",
                "fail",
                "sin reproductor",
                Some(Fix::Apt(vec!["pulseaudio-utils"])),
            )
        }
        if which("piper").is_some() {
            self.check(sec, "piper", "pass", "installed", None);
            let configured = read_setting(
                &self.home.join(".claude/hooks/cc-notify.conf"),
                "PIPER_VOICE",
                true,
            );
            let voice = if configured.is_empty() {
                VOICE.to_owned()
            } else {
                configured
            };
            let exists = self
                .home
                .join(".local/share/piper-voices")
                .join(format!("{voice}.onnx"))
                .is_file();
            self.check(
                sec,
                format!("voz {voice}"),
                if exists { "pass" } else { "warn" },
                if exists {
                    "descargada"
                } else {
                    "no descargada — cc-doctor --fix la baja"
                },
                Some(Fix::Voice(voice)),
            );
        } else {
            self.check(
                sec,
                "piper",
                "warn",
                "no instalado — voz deshabilitada (opcional)",
                None,
            )
        }
    }
    fn remote(&mut self) {
        let sec = "Móvil y remoto (opcional)";
        if which("tailscale").is_some() {
            let ok = succeeds("tailscale", &["status"]);
            self.check(
                sec,
                "tailscale",
                if ok { "pass" } else { "warn" },
                if ok {
                    "conectado"
                } else {
                    "instalado pero sin sesión — corre sudo tailscale up"
                },
                None,
            )
        } else {
            self.check(sec, "tailscale", "skip", "no instalado", None)
        }
        if which("cc-webterm").is_some() || which("comandos").is_some() {
            self.check(
                sec,
                "webterm Rust",
                "pass",
                "comandos webterm disponible",
                None,
            );
            for (name, var, default, path, detail) in [
                (
                    "cc-webterm-primary",
                    "CC_WEBTERM_PATH_PORT",
                    "4780",
                    "/term/token",
                    "responde en /term",
                ),
                (
                    "cc-webterm-fallback",
                    "CC_WEBTERM_PORT",
                    "4779",
                    "/token",
                    "responde en puerto local",
                ),
            ] {
                // Validate the port before constructing a fixed loopback URL.
                let port = env::var(var).unwrap_or_else(|_| default.into());
                let valid = port.parse::<u16>().is_ok_and(|p| p != 0);
                let ok = valid
                    && succeeds(
                        "curl",
                        &[
                            "-fsS",
                            "--max-time",
                            "2",
                            &format!("http://127.0.0.1:{port}{path}"),
                        ],
                    );
                self.check(
                    sec,
                    name,
                    if ok { "pass" } else { "warn" },
                    if ok {
                        detail
                    } else {
                        "apagado — cc-webterm para arrancar"
                    },
                    None,
                );
            }
        } else {
            self.check(sec, "webterm Rust", "skip", "no instalado", None)
        }
    }
}
fn yes(input: &mut impl BufRead, prompt: &str) -> bool {
    if env::var("CC_ASSUME_YES").as_deref() == Ok("1") {
        return true;
    }
    eprint!("{prompt} [Y/n] ");
    let _ = io::stderr().flush();
    let mut reply = String::new();
    // EOF is not an answer. An explicit blank line keeps the historical default.
    if !input.read_line(&mut reply).is_ok_and(|n| n > 0) {
        return false;
    }
    matches!(
        reply.trim_end_matches('\n'),
        "" | "y" | "Y" | "yes" | "Yes" | "YES" | "si" | "Si" | "SI" | "sí" | "Sí"
    )
}
fn apt(packages: &[&str], input: &mut impl BufRead, out: &mut String) -> bool {
    let missing = if let Ok(mock) = env::var("CC_MOCK_DPKG_MISSING") {
        mock.split_whitespace()
            .filter(|p| packages.contains(p))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    } else {
        packages
            .iter()
            .filter(|p| !succeeds("dpkg", &["-s", p]))
            .map(|p| (*p).to_owned())
            .collect()
    };
    if missing.is_empty() {
        return true;
    }
    let shown = missing.join(" ");
    eprintln!("Faltan paquetes: {shown}");
    if !yes(input, "¿Instalo ahora con sudo apt install?") {
        eprintln!("  Cancelado. Instala a mano:  sudo apt install {shown}");
        return false;
    }
    if env::var("CC_MOCK_APT").as_deref() == Ok("1") {
        out.push_str(&format!("[mock apt install {shown}]\n"));
        return true;
    }
    if !execute("sudo", &["apt", "update"], out) {
        return false;
    }
    let mut args = vec!["apt", "install", "-y"];
    args.extend(missing.iter().map(String::as_str));
    execute("sudo", &args, out)
}
fn voice(home: &Path, name: &str, out: &mut String) -> io::Result<bool> {
    if name != VOICE {
        out.push_str(&format!("  voz '{name}' desconocida — descarga manual\n"));
        return Ok(false);
    }
    let dest = home
        .join(".local/share/piper-voices")
        .join(format!("{name}.onnx"));
    fs::create_dir_all(
        dest.parent()
            .ok_or_else(|| io::Error::other("destino sin directorio"))?,
    )?;
    // Exclusive random private temp; cleanup on download/checksum/IO failures.
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let tmp = env::temp_dir().join(format!(
        "comandos-voice-{:032x}",
        u128::from_le_bytes(random)
    ));
    let _file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _cleanup = Temp(tmp.clone());
    out.push_str(&format!(
        "  Descargando {name} desde HuggingFace (~60MB)...\n"
    ));
    if !execute(
        "curl",
        &[
            "-fL",
            "--progress-bar",
            VOICE_URL,
            "-o",
            tmp.to_str()
                .ok_or_else(|| io::Error::other("ruta no UTF-8"))?,
        ],
        out,
    ) {
        out.push_str("  Descarga falló.\n");
        return Ok(false);
    }
    let mut file = fs::File::open(&tmp)?;
    let mut h = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        h.update(&buffer[..n]);
    }
    let got = format!("{:x}", h.finalize());
    if got != VOICE_SHA {
        out.push_str(&format!("  ✗ Checksum mismatch (got {got}, expected {VOICE_SHA}). Abortado.\n  Descarga manual: {VOICE_URL}\n"));
        return Ok(false);
    }
    // Atomic store helper works across a TMPDIR on a different filesystem.
    comandos_store::files::write_atomic(&dest, &fs::read(&tmp)?)?;
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o644))?;
    out.push_str(&format!("  ✓ Voz {name} instalada en {}\n", dest.display()));
    Ok(true)
}
fn render_json(row: &Check) {
    println!(
        "{}",
        serde_json::json!({"section":row.section,"check":row.name,"status":row.status,"detail":row.detail})
    );
}
pub fn main(args: &[String]) -> i32 {
    let (mut fix, mut json, mut sections) =
        (false, false, vec!["core", "desktop", "audio", "remote"]);
    for arg in args {
        match arg.as_str() {
            "--fix" => fix = true,
            "--json" => json = true,
            "-h" | "--help" => {
                print!("{HELP}");
                return 0;
            }
            "core" | "desktop" | "audio" | "remote" => sections = vec![arg.as_str()],
            _ => {
                eprintln!("sección desconocida: {arg}");
                return 2;
            }
        }
    }
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("cc-doctor: HOME no definido");
        return 1;
    };
    let mut doctor = Doctor::new(home);
    if !json {
        println!(
            "ComandOS doctor · plataforma: {}{}",
            doctor.platform,
            if doctor.codename.is_empty() {
                String::new()
            } else {
                format!(" ({})", doctor.codename)
            }
        );
    }
    for section in sections {
        match section {
            "core" => doctor.core(),
            "desktop" => doctor.desktop(),
            "audio" => doctor.audio(),
            "remote" => doctor.remote(),
            _ => unreachable!(),
        }
    }
    let mut last = "";
    let (mut ok, mut warn, mut fail) = (0, 0, 0);
    for row in &doctor.rows {
        match row.status {
            "pass" => ok += 1,
            "warn" => warn += 1,
            "fail" => fail += 1,
            _ => {}
        }
        if json {
            render_json(row);
            continue;
        }
        if row.section != last {
            println!("\n{}", row.section);
            last = row.section;
        }
        let (color, symbol) = match row.status {
            "pass" => (32, "✓"),
            "warn" => (33, "!"),
            "fail" => (31, "✗"),
            _ => (90, "·"),
        };
        println!(
            "  \x1b[{color}m{symbol}\x1b[0m {}{} {}",
            row.name,
            " ".repeat(36usize.saturating_sub(row.name.len())),
            row.detail
        );
    }
    if !json {
        println!("\nResumen: {ok} OK · {warn} WARN · {fail} FAIL");
        if fail > 0 {
            println!("Corre `cc-doctor --fix` para arreglar lo bloqueante.");
        }
    }
    let mut fix_failed = false;
    if fix {
        let mut any = false;
        let stdin = io::stdin();
        let mut input = stdin.lock();
        for row in &doctor.rows {
            if !matches!(row.status, "warn" | "fail") {
                continue;
            }
            let Some(action) = &row.fix else { continue };
            any = true;
            let mut log = if json {
                String::new()
            } else {
                format!("\n→ {}: {}\n", row.name, row.detail)
            };
            if !json {
                print!("{log}");
                let _ = io::stdout().flush();
                log.clear();
            }
            let accepted = yes(&mut input, &format!("  ¿Arreglo con: {}?", action.hint()));
            let success = if !accepted {
                log.push_str("  skip\n");
                true
            } else {
                match action {
                    Fix::Apt(packages) => apt(packages, &mut input, &mut log),
                    Fix::Exec(program, args) => execute(
                        program,
                        &args.iter().map(String::as_str).collect::<Vec<_>>(),
                        &mut log,
                    ),
                    Fix::Voice(name) => voice(&doctor.home, name, &mut log).unwrap_or_else(|e| {
                        eprintln!("cc-doctor: {e}");
                        false
                    }),
                }
            };
            fix_failed |= !success;
            if json {
                render_json(&Check {
                    section: "Arreglos",
                    name: row.name.clone(),
                    status: if !accepted {
                        "skip"
                    } else if success {
                        "pass"
                    } else {
                        "fail"
                    },
                    detail: log,
                    fix: None,
                });
            } else {
                print!("{log}");
            }
        }
        if json {
            render_json(&Check {
                section: "Arreglos",
                name: "verificación".into(),
                status: "warn",
                detail: if any {
                    "Vuelve a correr cc-doctor para verificar."
                } else {
                    "Nada que arreglar automáticamente. Vuelve a correr cc-doctor para verificar."
                }
                .into(),
                fix: None,
            });
        } else {
            if !any {
                println!("\nNada que arreglar automáticamente.");
            }
            println!("\nVuelve a correr cc-doctor para verificar.");
        }
    }
    i32::from(fail > 0 || fix_failed)
}
