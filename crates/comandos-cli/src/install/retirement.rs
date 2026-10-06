//! Explicit cleanup action; admission and archiving never install or stop anything.
use comandos_runtime::retirement::{self, CleanupCandidate, FindingKind, Manifest, Options};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Selection {
    pub paths: Vec<PathBuf>,
    pub denied: usize,
}

pub fn select(options: &Options, manifest: &Manifest) -> Result<Selection, String> {
    let paths: Vec<_> = manifest.rows.iter().map(|row| row.path.clone()).collect();
    let report = retirement::check(options, manifest, &paths).map_err(|e| e.to_string())?;
    let candidates: Vec<_> = report
        .artifacts
        .iter()
        .flat_map(|artifact| {
            artifact
                .findings
                .iter()
                .filter(|finding| finding.kind == FindingKind::HomeLink)
                .filter_map(|finding| {
                    let relative = finding.source.as_ref()?.strip_prefix(&options.home).ok()?;
                    Some(CleanupCandidate {
                        artifact: artifact.path.clone(),
                        relative_path: relative.into(),
                    })
                })
        })
        .collect();
    let selected = retirement::cleanup_selection(options, &report, &candidates);
    Ok(Selection {
        paths: selected.paths,
        denied: selected.denied.len(),
    })
}

fn inventory(home: &Path, repo: &Path) -> Result<Options, String> {
    let git = super::full::find("git").ok_or("git unavailable; cleanup census incomplete")?;
    let commit = super::full::capture(
        home,
        &git,
        vec![
            "-C".into(),
            repo.to_string_lossy().into_owned(),
            "rev-parse".into(),
            "HEAD".into(),
        ],
        Duration::from_secs(10),
    )?;
    if commit.code != Some(0) {
        return Err("repository commit unavailable".into());
    }
    let cron =
        super::full::find("crontab").ok_or("crontab unavailable; cleanup census incomplete")?;
    let output = super::full::capture(home, &cron, vec!["-l".into()], Duration::from_secs(10))?;
    let crontab = if output.code == Some(0) {
        String::from_utf8(output.stdout).map_err(|_| "crontab is not UTF-8")?
    } else if String::from_utf8_lossy(&output.stderr)
        .to_lowercase()
        .contains("no crontab")
    {
        String::new()
    } else {
        return Err("crontab inventory unavailable".into());
    };
    let transient = std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(
            || PathBuf::from(format!("/run/user/{}", nix::unistd::Uid::current())),
            PathBuf::from,
        )
        .join("systemd/transient");
    Ok(Options {
        repo: repo.into(),
        repo_live: repo.into(),
        home: home.into(),
        proc: "/proc".into(),
        transient,
        crontab,
        commit: String::from_utf8(commit.stdout)
            .map_err(|_| "commit is not UTF-8")?
            .trim()
            .into(),
        tracked: None,
    })
}

pub fn run(home: &Path, repo: &Path, dry: bool) -> Result<i32, String> {
    super::release::check_app_parents(&repo.join("placeholder"))?;
    if !repo.is_dir() {
        return Err("cleanup repository must exist".into());
    }
    let manifest_path = repo.join("docs/verification/retirement.json");
    let manifest =
        Manifest::load(&manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    if !manifest.rows.iter().any(|row| row.status == "retired") {
        println!("cleanup blocked: no artifact has completed retirement; HOME unchanged");
        return Ok(1);
    }
    let options = inventory(home, repo)?;
    let selected = select(&options, &manifest)?;
    for relative in &selected.paths {
        println!(
            "{}: {}",
            if dry {
                "dry-run archive"
            } else {
                "admitted archive"
            },
            home.join(relative).display()
        );
    }
    if selected.denied != 0 {
        println!(
            "cleanup blocked: {} referenced paths lack admission; HOME unchanged",
            selected.denied
        );
        return Ok(1);
    }
    if selected.paths.is_empty() {
        println!("no legacy links have been admitted for archiving");
        return Ok(0);
    }
    let backup = super::cleanup::stage_admitted(home, &selected.paths, dry, &mut || {
        let current = Manifest::load(&manifest_path).map_err(|e| e.to_string())?;
        let now = select(&inventory(home, repo)?, &current)?;
        if now.denied != 0 || now.paths != selected.paths {
            return Err(
                "retirement admission changed before archiving; no legacy paths moved".into(),
            );
        }
        Ok(())
    })?;
    if let Some(manifest) = backup {
        println!("restore manifest: {}", manifest.display())
    }
    Ok(0)
}
