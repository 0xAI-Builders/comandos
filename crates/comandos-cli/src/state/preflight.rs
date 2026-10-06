//! Censo inyectable. Una lectura incompleta bloquea el cambio de modo.
use crate::install::manifest::{STATE_PROTOCOL, release_protocol};
use comandos_store::domains::catalog::{DOMAINS, Domain, domain};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};
#[derive(Debug)]
pub struct WriterProc {
    pub pid: i32,
    pub exe: PathBuf,
    pub argv: Vec<String>,
    pub protocol: u32,
    pub python_repo: bool,
}
fn blocked(pid: i32, path: &Path, reason: impl std::fmt::Display) -> WriterProc {
    WriterProc {
        pid,
        exe: path.to_owned(),
        argv: vec![format!("preflight incompleto: {reason}")],
        protocol: 0,
        python_repo: false,
    }
}
/// Nunca ignora errores de /proc; se representan como escritores sin capacidad.
pub fn domain_writers(proc_root: &Path, home: &Path, repo: &Path, name: &str) -> Vec<WriterProc> {
    let Some(domain) = domain(name) else {
        return vec![blocked(0, proc_root, format!("dominio desconocido {name}"))];
    };
    let entries = match fs::read_dir(proc_root) {
        Ok(e) => e,
        Err(e) => return vec![blocked(0, proc_root, e)],
    };
    let mut writers = vec![];
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                writers.push(blocked(0, proc_root, e));
                continue;
            }
        };
        let raw = entry.file_name();
        let Some(raw) = raw.to_str() else {
            writers.push(blocked(0, proc_root, "entrada no UTF-8"));
            continue;
        };
        if !raw.bytes().all(|b| b.is_ascii_digit()) || raw.is_empty() {
            continue;
        }
        let pid = match raw.parse::<i32>() {
            Ok(pid) => pid,
            Err(e) => {
                writers.push(blocked(0, &entry.path(), e));
                continue;
            }
        };
        match inspect(pid, &entry.path(), home, repo, domain) {
            Ok(Some(writer)) => writers.push(writer),
            Ok(None) => {}
            Err(reason) => writers.push(blocked(pid, &entry.path(), reason)),
        }
    }
    writers.sort_by_key(|w| w.pid);
    writers
}
fn inspect(
    pid: i32,
    path: &Path,
    home: &Path,
    repo: &Path,
    domain: &Domain,
) -> Result<Option<WriterProc>, String> {
    let exe = fs::read_link(path.join("exe")).map_err(|e| format!("exe ilegible: {e}"))?;
    let bytes = fs::read(path.join("cmdline")).map_err(|e| format!("cmdline ilegible: {e}"))?;
    let argv: Vec<String> = bytes
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8(s.to_vec()).map_err(|e| format!("argv no UTF-8: {e}")))
        .collect::<Result<_, _>>()?;
    // Los procesos sin argv no permiten demostrar que no escriben ningún dominio.
    if argv.is_empty() {
        return Err("argv vacío; escritor ambiguo".into());
    }
    let first = argv.first().map(String::as_str).unwrap_or_default();
    let exe_name = basename(&exe);
    let is_python =
        basename(Path::new(first)).starts_with("python") || exe_name.starts_with("python");
    let mut python_repo = false;
    let mut python_script = None;
    if is_python {
        let entrypoint = python_entrypoint(&argv)?;
        let candidate = Path::new(entrypoint);
        let absolute = if candidate.is_absolute() {
            candidate.to_owned()
        } else {
            let cwd = fs::read_link(path.join("cwd")).map_err(|e| format!("cwd ilegible: {e}"))?;
            cwd.join(candidate)
        };
        let resolved = fs::canonicalize(&absolute)
            .or_else(|_| comandos_store::domains::catalog::control_path_identity(&absolute))
            .map_err(|e| format!("entrypoint Python sin identidad: {e}"))?;
        let repo = fs::canonicalize(repo).map_err(|e| format!("repo sin identidad: {e}"))?;
        if let Ok(relative) = resolved.strip_prefix(&repo) {
            python_repo = true;
            // El registro del plan identifica ejecutables en bin, no nombres de datos.
            if relative.parent() == Some(Path::new("bin")) {
                python_script = Some(basename(&resolved));
            }
        }
    }
    let rust = rust_command(&exe, &argv);
    let rust_match = rust
        .as_deref()
        .is_some_and(|command| matches_rust(domain, command));
    let python_match = python_repo
        && python_script.as_deref().is_some_and(|script| {
            domain.python_writers.contains(&script)
                || (script == "cc-dash" && domain.rust_writers.contains(&"dash"))
        });
    // Solo el entrypoint puede demostrar el alcance. Sus argumentos no son escritores.
    let ambiguous_python = python_repo
        && !python_script.as_deref().is_some_and(|script| {
            script == "cc-dash" || DOMAINS.iter().any(|d| d.python_writers.contains(&script))
        });
    let in_release = release_dir(&exe, home);
    let ambiguous_rust = (exe_name == "comandos"
        || basename(Path::new(first)) == "comandos"
        || in_release.is_some())
        && rust.is_none();
    if !(rust_match || python_match || ambiguous_python || ambiguous_rust) {
        return Ok(None);
    }
    let protocol = if ambiguous_rust || ambiguous_python {
        0
    } else {
        in_release.as_deref().map(release_protocol).unwrap_or(0)
    };
    Ok(Some(WriterProc {
        pid,
        exe,
        argv,
        protocol,
        python_repo,
    }))
}
/// Consume las opciones del intérprete y devuelve únicamente el script ejecutado.
/// Código, módulos, stdin y opciones desconocidas no permiten demostrar un alcance.
fn python_entrypoint(argv: &[String]) -> Result<&str, String> {
    let mut args = argv.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "-m" | "-" => return Err("entrypoint Python dinámico; alcance ambiguo".into()),
            "--" => {
                return args
                    .next()
                    .map(String::as_str)
                    .ok_or_else(|| "entrypoint Python ausente".into());
            }
            "-W" | "-X" | "--check-hash-based-pycs" => {
                args.next()
                    .ok_or_else(|| format!("opción Python incompleta: {arg}"))?;
            }
            _ if !arg.starts_with('-') => return Ok(arg),
            _ if arg.starts_with("-c") || arg.starts_with("-m") => {
                return Err("entrypoint Python dinámico; alcance ambiguo".into());
            }
            _ if arg.starts_with("-W")
                || arg.starts_with("-X")
                || arg.starts_with("--check-hash-based-pycs=") => {}
            _ if arg.strip_prefix('-').is_some_and(|flags| {
                !flags.is_empty() && flags.chars().all(|flag| "bBdEiIOPqsSuvx".contains(flag))
            }) => {}
            _ => return Err(format!("opción Python sin alcance demostrado: {arg}")),
        }
    }
    Err("entrypoint Python ausente; alcance ambiguo".into())
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|v| {
            v.to_string_lossy()
                .trim_end_matches(" (deleted)")
                .to_string()
        })
        .unwrap_or_default()
}
fn rust_command(exe: &Path, argv: &[String]) -> Option<String> {
    let name = basename(exe);
    let invoked = argv
        .first()
        .map(|s| basename(Path::new(s)))
        .unwrap_or_default();
    if matches!(name.as_str(), "comandos-app" | "comandos-app-mac") {
        return Some(name);
    }
    if name == "comandos"
        && let Some(prefix) = crate::dispatch::alias_prefix(&invoked)
    {
        return Some(prefix.join(" "));
    }
    if name != "comandos" && invoked != "comandos" {
        return None;
    }
    let command = argv.get(1)?;
    let known = [
        "dash", "snapshot", "next", "hook", "acp", "ext", "events", "codex", "install", "keys",
        "web", "browser", "state", "doctor", "agents", "mobile", "x", "raise",
    ];
    if !known.contains(&command.as_str()) {
        return None;
    }
    if command == "hook" {
        return argv.get(2).map(|kind| format!("hook {kind}"));
    }
    Some(command.to_owned())
}
fn matches_rust(domain: &Domain, command: &str) -> bool {
    domain.rust_writers.iter().any(|pattern| {
        command == *pattern
            || command
                .strip_prefix(pattern)
                .is_some_and(|rest| rest.starts_with(' '))
    })
}
fn release_dir(exe: &Path, home: &Path) -> Option<PathBuf> {
    let base = home.join(".local/share/comandos/releases");
    let stripped = exe.strip_prefix(&base).ok()?;
    let mut parts = stripped.components();
    let id = parts.next()?;
    let binary = parts.next()?;
    if !matches!(id, Component::Normal(_))
        || !matches!(binary, Component::Normal(_))
        || parts.any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    Some(base.join(id.as_os_str()))
}
pub fn can_unify(writers: &[WriterProc]) -> Result<(), Vec<String>> {
    let reasons: Vec<_> = writers
        .iter()
        .filter(|w| w.python_repo || w.protocol != STATE_PROTOCOL)
        .map(|w| {
            if w.python_repo {
                format!("pid {} Python del repo ({})", w.pid, w.argv.join(" "))
            } else {
                format!(
                    "pid {} ({}, release {}, protocolo {}; requerido {})",
                    w.pid,
                    w.argv.join(" "),
                    w.exe
                        .parent()
                        .and_then(Path::file_name)
                        .map(|v| v.to_string_lossy())
                        .unwrap_or_default(),
                    w.protocol,
                    STATE_PROTOCOL
                )
            }
        })
        .collect();
    if reasons.is_empty() {
        Ok(())
    } else {
        Err(reasons)
    }
}
