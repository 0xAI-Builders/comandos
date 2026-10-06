//! Read-only retirement admission; scanners never archive, stop or migrate anything.
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    HomeLink,
    Unit,
    AgentConfig,
    LiveProcess,
    LiveImporter,
    TmuxConf,
    Crontab,
    ReferencedByTracked,
    MissingReplacement,
}
#[derive(Debug, Clone)]
pub struct Finding {
    pub kind: FindingKind,
    pub source: Option<PathBuf>,
    pub pid: Option<u32>,
    pub reference: String,
    pub detail: String,
}
impl Finding {
    fn json(&self) -> Value {
        json!({"kind":format!("{:?}",self.kind),"path":self.source,"pid":self.pid,"reference":self.reference,"detail":self.detail})
    }
}
#[derive(Debug, Clone)]
pub struct Verification {
    pub commit: String,
    pub passed: bool,
    pub standalone: bool,
}
#[derive(Debug, Clone)]
pub struct Row {
    pub path: String,
    pub rust: Option<String>,
    pub phase: String,
    pub verified_by: Vec<String>,
    pub status: String,
    pub verification: Option<Verification>,
}
#[derive(Debug, Clone)]
pub struct Manifest {
    pub rows: Vec<Row>,
}
fn invalid(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, text)
}
fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|p| matches!(p, Component::Normal(_)))
}
impl Manifest {
    pub fn load(path: &Path) -> io::Result<Self> {
        Self::from_value(&serde_json::from_slice(&fs::read(path)?)?)
    }
    pub fn from_value(value: &Value) -> io::Result<Self> {
        if value["version"] != 1 {
            return Err(invalid("unsupported retirement manifest version"));
        }
        let mut rows = Vec::new();
        let mut names = BTreeSet::new();
        for value in value["rows"]
            .as_array()
            .ok_or_else(|| invalid("missing retirement rows"))?
        {
            let path = value["path"]
                .as_str()
                .ok_or_else(|| invalid("missing retirement path"))?
                .to_owned();
            if !relative(Path::new(&path)) || !names.insert(path.clone()) {
                return Err(invalid("invalid or duplicate retirement path"));
            }
            let status = value["status"].as_str().unwrap_or("pending").to_owned();
            if !matches!(status.as_str(), "pending" | "retired") {
                return Err(invalid("invalid retirement status"));
            }
            let verified_by = value["verified_by"]
                .as_array()
                .ok_or_else(|| invalid("missing verified_by"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("invalid test reference"))
                })
                .collect::<io::Result<Vec<_>>>()?;
            let verification = value
                .get("verification")
                .filter(|v| !v.is_null())
                .map(|v| {
                    Ok::<Verification, io::Error>(Verification {
                        commit: v["commit"]
                            .as_str()
                            .ok_or_else(|| invalid("missing verification commit"))?
                            .into(),
                        passed: v["passed"].as_bool().unwrap_or(false),
                        standalone: v["standalone"].as_bool().unwrap_or(false),
                    })
                })
                .transpose()?;
            rows.push(Row {
                path,
                rust: value["rust"].as_str().map(str::to_owned),
                phase: value["phase"].as_str().unwrap_or("").into(),
                verified_by,
                status,
                verification,
            });
        }
        Ok(Self { rows })
    }
}
#[derive(Debug, Clone)]
pub struct Options {
    pub repo: PathBuf,
    pub repo_live: PathBuf,
    pub home: PathBuf,
    pub proc: PathBuf,
    pub transient: PathBuf,
    /// Injected by the CLI's read-only `crontab -l`, or by a private fixture.
    pub crontab: String,
    pub commit: String,
    /// None uses `git ls-files -z`; fixtures can supply their exact tracked tree.
    pub tracked: Option<Vec<String>>,
}
#[derive(Debug, Clone)]
pub struct ArtifactReport {
    pub path: String,
    pub status: String,
    pub replacement_verified: bool,
    pub standalone: bool,
    pub findings: Vec<Finding>,
}
#[derive(Debug, Clone)]
pub struct Report {
    pub artifacts: Vec<ArtifactReport>,
}
impl Report {
    pub fn clean(&self) -> bool {
        self.artifacts.iter().all(|a| a.findings.is_empty())
    }
    pub fn json(&self) -> Value {
        json!({"version":1,"clean":self.clean(),"artifacts":self.artifacts.iter().map(|a| json!({"path":a.path,"status":a.status,"replacement_verified":a.replacement_verified,"standalone":a.standalone,"findings":a.findings.iter().map(Finding::json).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
}
fn tracked(opts: &Options) -> io::Result<Vec<String>> {
    if let Some(files) = &opts.tracked {
        return Ok(files.clone());
    }
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&opts.repo)
        .output()?;
    if !out.status.success() {
        return Err(invalid("git tracked-file inventory unavailable"));
    }
    let text = String::from_utf8(out.stdout).map_err(|_| invalid("non-UTF8 tracked path"))?;
    Ok(text
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}
fn text(path: &Path) -> io::Result<Option<String>> {
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > 32 * 1024 * 1024 {
        return Err(invalid("retirement source exceeds scan limit"));
    }
    Ok(String::from_utf8(fs::read(path)?).ok())
}
fn walk(root: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if metadata.is_dir() {
        for entry in fs::read_dir(root)? {
            walk(&entry?.path(), out)?;
        }
    } else {
        out.push(root.to_owned());
    }
    Ok(())
}
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            _ => out.push(c.as_os_str()),
        }
    }
    out
}
fn resolved_link(path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_owned();
    for _ in 0..40 {
        match fs::read_link(&current) {
            Ok(target) => {
                current = normalize(&if target.is_absolute() {
                    target
                } else {
                    current.parent().unwrap_or(Path::new("/")).join(target)
                });
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::NotFound
                ) =>
            {
                return Ok(fs::canonicalize(&current).unwrap_or(current));
            }
            Err(e) => return Err(e),
        }
    }
    Err(invalid("symlink cycle in retirement source"))
}
fn finding(
    kind: FindingKind,
    source: Option<PathBuf>,
    pid: Option<u32>,
    artifact: &str,
    detail: &str,
) -> Finding {
    Finding {
        kind,
        source,
        pid,
        reference: artifact.into(),
        detail: detail.into(),
    }
}
fn mentions(body: &str, artifact: &str, absolute: &Path, bin_name: bool) -> bool {
    body.contains(artifact)
        || body.contains(absolute.to_string_lossy().as_ref())
        || (bin_name
            && artifact.starts_with("bin/")
            && Path::new(artifact)
                .file_name()
                .and_then(|p| p.to_str())
                .is_some_and(|name| body.contains(name)))
}
fn evidence_exists(repo: &Path, evidence: &str, files: &[String]) -> io::Result<bool> {
    let parts = evidence.split("::").collect::<Vec<_>>();
    let [krate, file, name] = parts.as_slice() else {
        return Ok(false);
    };
    if ![krate, file, name].iter().all(|p| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '*'))
    }) {
        return Ok(false);
    }
    let prefix = if *krate == "xtask" {
        "xtask/tests/".into()
    } else {
        format!("crates/{krate}/tests/")
    };
    let pat = |s: &str| regex::escape(s).replace("\\*", ".*");
    let file_match = Regex::new(&format!("^{}{}\\.rs$", regex::escape(&prefix), pat(file)))
        .map_err(|_| invalid("test path pattern"))?;
    let function = Regex::new(&format!(
        "(?m)^[ \\t]*(?:async[ \\t]+)?fn[ \\t]+{}[ \\t]*\\(",
        pat(name)
    ))
    .map_err(|_| invalid("test function pattern"))?;
    for path in files.iter().filter(|p| file_match.is_match(p)) {
        if text(&repo.join(path))?.is_some_and(|t| {
            function.is_match(&t) && (t.contains("#[test]") || t.contains("#[tokio::test]"))
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}
fn imports(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in body.lines() {
        let line = line.trim_start();
        let tokens = if let Some(rest) = line.strip_prefix("import ") {
            rest
        } else if let Some(rest) = line.strip_prefix("from ") {
            rest
        } else {
            continue;
        };
        for token in tokens.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.')))
        {
            if !matches!(token, "" | "import" | "as")
                && let Some(part) = token.rsplit('.').find(|p| !p.is_empty())
            {
                out.insert(part.into());
            }
        }
    }
    out
}
fn reachable(
    start: &str,
    target: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    dynamic: &BTreeSet<String>,
) -> bool {
    let mut pending = vec![start.to_owned()];
    let mut seen = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if path == target || dynamic.contains(&path) {
            return true;
        }
        if seen.insert(path.clone())
            && let Some(next) = graph.get(&path)
        {
            pending.extend(next.iter().cloned());
        }
    }
    false
}
pub fn check(opts: &Options, manifest: &Manifest, paths: &[String]) -> io::Result<Report> {
    for root in [&opts.repo, &opts.repo_live, &opts.home, &opts.proc] {
        if !root.is_absolute() || !root.is_dir() {
            return Err(invalid("retirement scan roots must exist and be absolute"));
        }
    }
    let files = tracked(opts)?;
    if files.iter().any(|p| !relative(Path::new(p))) {
        return Err(invalid("invalid tracked path"));
    }
    let retiring = paths.iter().cloned().collect::<BTreeSet<_>>();
    if paths.is_empty() || paths.iter().any(|p| !relative(Path::new(p))) {
        return Err(invalid("at least one relative artifact path is required"));
    }
    let mut bodies = BTreeMap::new();
    for path in &files {
        if !path.starts_with("docs/")
            && !path.ends_with(".md")
            && let Some(body) = text(&opts.repo.join(path))?
        {
            bodies.insert(path.clone(), body);
        }
    }
    let mut modules: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in bodies
        .keys()
        .filter(|p| p.starts_with("lib/") || p.starts_with("bin/"))
    {
        if let Some(name) = Path::new(path).file_stem().and_then(|p| p.to_str()) {
            modules.entry(name.into()).or_default().push(path.clone());
        }
    }
    let mut graph = BTreeMap::new();
    let mut dynamic = BTreeSet::new();
    for (path, body) in bodies
        .iter()
        .filter(|(p, _)| p.starts_with("bin/") || p.starts_with("lib/"))
    {
        let next = imports(body)
            .into_iter()
            .filter_map(|name| modules.get(&name))
            .flatten()
            .cloned()
            .collect();
        graph.insert(path.clone(), next);
        if body.contains("__import__(")
            || body.contains("importlib.import_module")
            || body.contains("exec(")
        {
            dynamic.insert(path.clone());
        }
    }
    let mut home_files = Vec::new();
    for path in [
        ".local/bin",
        ".claude/hooks",
        ".config/systemd/user",
        ".config/kitty",
        ".local/share/applications",
        ".tmux.conf",
    ] {
        walk(&opts.home.join(path), &mut home_files)?;
    }
    let mut links = Vec::new();
    for path in &home_files {
        if fs::symlink_metadata(path)?.is_symlink() {
            links.push((path.clone(), resolved_link(path)?));
        }
    }
    let mut units = Vec::new();
    walk(&opts.home.join(".config/systemd/user"), &mut units)?;
    walk(&opts.transient, &mut units)?;
    let mut configs = Vec::new();
    for path in [
        ".codex/config.toml",
        ".gemini/settings.json",
        ".gemini/config/hooks.json",
        ".gemini/antigravity-cli/settings.json",
        ".config/opencode/opencode.json",
        ".config/opencode/opencode.jsonc",
        ".claude/settings.json",
        ".claude-accounts",
    ] {
        walk(&opts.home.join(path), &mut configs)?;
    }
    let mut processes = Vec::new();
    for entry in fs::read_dir(&opts.proc)? {
        let entry = entry?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        match fs::read(entry.path().join("cmdline")) {
            Ok(body) => {
                let cwd = match fs::read_link(entry.path().join("cwd")) {
                    Ok(path) => Some(path),
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::InvalidInput
                        ) =>
                    {
                        None
                    }
                    Err(e) => return Err(e),
                };
                processes.push((
                    pid,
                    cwd,
                    body.split(|b| *b == 0)
                        .filter(|s| !s.is_empty())
                        .map(|s| String::from_utf8_lossy(s).into_owned())
                        .collect::<Vec<_>>(),
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    let mut artifacts = Vec::new();
    for path in paths {
        let live = fs::canonicalize(&opts.repo_live)?;
        let absolute = normalize(&live.join(path));
        let mut findings = Vec::new();
        let row = manifest.rows.iter().find(|r| &r.path == path);
        let mut replacement_verified = row.is_some_and(|r| {
            r.rust.as_ref().is_some_and(|s| !s.trim().is_empty())
                && !r.verified_by.is_empty()
                && r.verification
                    .as_ref()
                    .is_some_and(|v| v.passed && !opts.commit.is_empty() && v.commit == opts.commit)
        });
        if let Some(row) = row {
            for test in &row.verified_by {
                if !evidence_exists(&opts.repo, test, &files)? {
                    replacement_verified = false;
                    findings.push(finding(
                        FindingKind::MissingReplacement,
                        None,
                        None,
                        path,
                        "declared replacement test does not exist",
                    ));
                }
            }
        }
        if !replacement_verified {
            findings.push(finding(
                FindingKind::MissingReplacement,
                None,
                None,
                path,
                "replacement and passing verification for this commit are required",
            ));
        }
        let mut aliases = Vec::new();
        for (link, target) in &links {
            if target == &absolute || absolute.starts_with(target) {
                aliases.extend(aliases_for(&[(link.clone(), target.clone())], &absolute));
                findings.push(finding(
                    FindingKind::HomeLink,
                    Some(link.clone()),
                    None,
                    path,
                    "HOME link resolves into the artifact",
                ));
            }
        }
        for source in &units {
            if source.extension().is_some_and(|s| s == "service")
                && text(source)?.is_some_and(|body| mentions(&body, path, &absolute, true))
            {
                findings.push(finding(
                    FindingKind::Unit,
                    Some(source.clone()),
                    None,
                    path,
                    "unit still references the artifact",
                ));
            }
        }
        for source in &configs {
            if text(source)?.is_some_and(|body| {
                mentions(&body, path, &absolute, false)
                    || aliases
                        .iter()
                        .any(|p| body.contains(p.to_string_lossy().as_ref()))
            }) {
                findings.push(finding(
                    FindingKind::AgentConfig,
                    Some(source.clone()),
                    None,
                    path,
                    "agent configuration still references the artifact",
                ));
            }
        }
        for source in [
            opts.home.join(".tmux.conf"),
            opts.repo.join("config/tmux.conf"),
        ] {
            if text(&source)?.is_some_and(|body| mentions(&body, path, &absolute, true)) {
                findings.push(finding(
                    FindingKind::TmuxConf,
                    Some(source),
                    None,
                    path,
                    "tmux configuration still references the artifact",
                ));
            }
        }
        if mentions(&opts.crontab, path, &absolute, true) {
            findings.push(finding(
                FindingKind::Crontab,
                None,
                None,
                path,
                "crontab still references the artifact",
            ));
        }
        for (pid, cwd, args) in &processes {
            if args.iter().any(|a| {
                a == absolute.to_string_lossy().as_ref()
                    || cwd
                        .as_ref()
                        .is_some_and(|cwd| normalize(&cwd.join(a)) == absolute)
                    || aliases.iter().any(|p| a == p.to_string_lossy().as_ref())
            }) {
                findings.push(finding(
                    FindingKind::LiveProcess,
                    Some(opts.proc.join(pid.to_string()).join("cmdline")),
                    Some(*pid),
                    path,
                    "live command still names the artifact or its HOME link",
                ));
            }
            if path.starts_with("lib/")
                && path.ends_with(".py")
                && args
                    .first()
                    .and_then(|a| Path::new(a).file_name())
                    .is_some_and(|a| a.to_string_lossy().starts_with("python"))
            {
                for script in graph.keys() {
                    let script_abs = live.join(script);
                    let named = args.iter().any(|a| {
                        a == script_abs.to_string_lossy().as_ref()
                            || cwd
                                .as_ref()
                                .is_some_and(|cwd| normalize(&cwd.join(a)) == script_abs)
                            || aliases_for(&links, &script_abs)
                                .iter()
                                .any(|p| a == p.to_string_lossy().as_ref())
                    });
                    if named && reachable(script, path, &graph, &dynamic) {
                        findings.push(finding(
                            FindingKind::LiveImporter,
                            Some(script_abs),
                            Some(*pid),
                            path,
                            "live Python entry point can import the module",
                        ));
                    }
                }
            }
        }
        for (source, body) in &bodies {
            if !retiring.contains(source) && mentions(body, path, &absolute, false) {
                findings.push(finding(
                    FindingKind::ReferencedByTracked,
                    Some(opts.repo.join(source)),
                    None,
                    path,
                    "tracked code or test still references the artifact",
                ));
            }
        }
        artifacts.push(ArtifactReport {
            path: path.clone(),
            status: row.map_or("pending", |r| r.status.as_str()).into(),
            replacement_verified,
            standalone: row
                .and_then(|r| r.verification.as_ref())
                .is_some_and(|v| v.standalone),
            findings,
        });
    }
    Ok(Report { artifacts })
}
fn aliases_for(links: &[(PathBuf, PathBuf)], target: &Path) -> Vec<PathBuf> {
    links
        .iter()
        .filter_map(|(link, resolved)| {
            target
                .strip_prefix(resolved)
                .ok()
                .map(|tail| link.join(tail))
        })
        .collect()
}
#[derive(Debug, Clone)]
pub struct CleanupCandidate {
    pub artifact: String,
    pub relative_path: PathBuf,
}
#[derive(Debug)]
pub struct CleanupSelection {
    pub paths: Vec<PathBuf>,
    pub denied: Vec<Finding>,
}
/// Admit only explicit mappings. HOME links in this exact cleanup batch may be removed;
/// every other reference and live process remains a blocker.
pub fn cleanup_selection(
    opts: &Options,
    report: &Report,
    candidates: &[CleanupCandidate],
) -> CleanupSelection {
    let mut selection = CleanupSelection {
        paths: Vec::new(),
        denied: Vec::new(),
    };
    for candidate in candidates {
        let artifact = report
            .artifacts
            .iter()
            .find(|a| a.path == candidate.artifact);
        let protected = candidate.relative_path.components().any(|c| matches!(c,Component::Normal(s) if matches!(s.to_str(),Some("providers.env"|"dash-token"|"cc-notify.conf"|"session-handoffs"|"catalog.json"|"credentials.json"|"rollback"|"releases"))));
        let special = candidate.relative_path.components().any(|c| matches!(c,Component::Normal(s) if matches!(s.to_str(),Some("dash"|"extensions-venv"))));
        // A caller-provided label alone cannot authorize archiving an unrelated HOME file.
        let mapped = artifact.is_some_and(|a| {
            a.findings.iter().any(|f| {
                f.kind == FindingKind::HomeLink
                    && f.source
                        .as_ref()
                        .is_some_and(|p| *p == opts.home.join(&candidate.relative_path))
            })
        });
        let valid = relative(&candidate.relative_path)
            && mapped
            && !protected
            && artifact.is_some_and(|a| {
                a.status == "retired" && a.replacement_verified && (!special || a.standalone)
            });
        if !valid {
            selection.denied.push(finding(FindingKind::MissingReplacement,Some(opts.home.join(&candidate.relative_path)),None,&candidate.artifact,"cleanup requires retired status, current replacement verification and an admitted HOME path"));
            continue;
        }
        let Some(artifact) = artifact else {
            continue;
        };
        let blockers = artifact
            .findings
            .iter()
            .filter(|f| {
                !(f.kind == FindingKind::HomeLink
                    && f.source.as_ref().is_some_and(|source| {
                        candidates.iter().any(|c| {
                            c.artifact == candidate.artifact
                                && relative(&c.relative_path)
                                && !c.relative_path.components().any(|p| matches!(p,Component::Normal(s) if matches!(s.to_str(),Some("providers.env"|"dash-token"|"cc-notify.conf"|"session-handoffs"|"catalog.json"|"credentials.json"|"rollback"|"releases"))))
                                && (artifact.standalone || !c.relative_path.components().any(|p| matches!(p,Component::Normal(s) if matches!(s.to_str(),Some("dash"|"extensions-venv")))))
                                && opts.home.join(&c.relative_path) == *source
                        })
                    }))
            })
            .cloned()
            .collect::<Vec<_>>();
        if blockers.is_empty() {
            selection.paths.push(candidate.relative_path.clone());
        } else {
            selection.denied.extend(blockers);
        }
    }
    selection.paths.sort();
    selection.paths.dedup();
    selection
}
