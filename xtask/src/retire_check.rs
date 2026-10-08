//! Read-only R1 CLI; the installer consumes the same runtime admission API.
pub use comandos_runtime::retirement::*;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub struct Args {
    pub options: Options,
    pub manifest: PathBuf,
    pub paths: Vec<String>,
    crontab_file: Option<PathBuf>,
}
fn value(args: &[String], i: &mut usize) -> Result<PathBuf, String> {
    *i += 1;
    args.get(*i)
        .map(PathBuf::from)
        .ok_or_else(|| "missing option value".into())
}
pub fn parse(args: &[String]) -> Result<Args, String> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("repository root missing")?
        .to_path_buf();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME missing")?;
    let transient = std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(
            || PathBuf::from(format!("/run/user/{}", nix::unistd::Uid::current())),
            PathBuf::from,
        )
        .join("systemd/transient");
    let mut options = Options {
        repo: repo.clone(),
        repo_live: repo,
        home,
        proc: "/proc".into(),
        transient,
        crontab: String::new(),
        commit: String::new(),
        tracked: None,
    };
    let mut manifest = None;
    let mut paths = Vec::new();
    let mut crontab_file = None;
    let mut i = 0;
    while i < args.len() {
        match args.get(i).map(String::as_str) {
            Some("--home") => options.home = value(args, &mut i)?,
            Some("--proc") => options.proc = value(args, &mut i)?,
            Some("--repo") => options.repo = value(args, &mut i)?,
            Some("--repo-live") => options.repo_live = value(args, &mut i)?,
            Some("--transient") => options.transient = value(args, &mut i)?,
            Some("--manifest") => manifest = Some(value(args, &mut i)?),
            Some("--crontab-file") => crontab_file = Some(value(args, &mut i)?),
            Some("--json") => {}
            Some(arg) if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            Some(path) => paths.push(path.to_owned()),
            None => {}
        }
        i += 1;
    }
    if paths.is_empty() {
        return Err("uso: cargo xtask retire-check <artefacto>… [--home DIR] [--proc DIR] [--repo-live DIR] [--json]".into());
    }
    if options.repo_live
        == Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("repository root missing")?
    {
        options.repo_live = options.repo.clone();
    }
    for path in &mut paths {
        if Path::new(path).is_absolute() {
            *path = Path::new(path)
                .strip_prefix(&options.repo)
                .map_err(|_| "artifact outside repo")?
                .to_string_lossy()
                .into_owned();
        }
    }
    let manifest =
        manifest.unwrap_or_else(|| options.repo.join("docs/verification/retirement.json"));
    Ok(Args {
        options,
        manifest,
        paths,
        crontab_file,
    })
}
pub fn run(mut args: Args) -> io::Result<Report> {
    args.options.crontab = if let Some(path) = args.crontab_file {
        fs::read_to_string(path)?
    } else {
        let out = Command::new("crontab")
            .arg("-l")
            .stdin(Stdio::null())
            .output()?;
        if out.status.success() {
            String::from_utf8(out.stdout).map_err(|_| io::Error::other("non-UTF8 crontab"))?
        } else if String::from_utf8_lossy(&out.stderr)
            .to_lowercase()
            .contains("no crontab")
        {
            String::new()
        } else {
            return Err(io::Error::other("crontab inventory unavailable"));
        }
    };
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&args.options.repo)
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other("repository commit unavailable"));
    }
    args.options.commit = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    check(&args.options, &Manifest::load(&args.manifest)?, &args.paths)
}
pub fn main(args: &[String]) -> i32 {
    let args = match parse(args) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    match run(args) {
        Ok(report) => match serde_json::to_string_pretty(&report.json()) {
            Ok(json) => {
                println!("{json}");
                if report.clean() { 0 } else { 1 }
            }
            Err(_) => {
                eprintln!("error: retirement report unavailable");
                2
            }
        },
        Err(error) => {
            eprintln!("error: {error}");
            2
        }
    }
}
