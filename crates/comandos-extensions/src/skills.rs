//! Complete skill resources with canonical links and loss-checked reinstalls.
use crate::{
    Result, config,
    mutations::{self, Mutation},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};
pub fn roots(home: &Path) -> Result<Vec<PathBuf>> {
    let mut result = [
        ".agents/skills",
        ".claude/skills",
        ".codex/skills",
        ".grok/skills",
        ".config/opencode/skills",
        ".opencode/skills",
        ".gemini/config/skills",
    ]
    .iter()
    .map(|p| home.join(p))
    .collect::<Vec<_>>();
    for kind in ["claude", "codex", "grok"] {
        for root in crate::catalog::accounts(home, kind)? {
            result.push(root.join("skills"));
        }
    }
    Ok(result)
}
fn list(path: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(path)
        .map_err(|_| config::err(path))?
        .map(|e| e.map(|e| e.path()).map_err(|_| config::err(path)))
        .collect::<Result<Vec<_>>>()?;
    paths.sort();
    Ok(paths)
}
fn name(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| config::err(path))
}
fn valid(path: &Path) -> bool {
    name(path).is_ok_and(|n| !n.starts_with('.')) && path.join("SKILL.md").is_file()
}
fn resolve(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|_| config::err(path));
    }
    let parent = path.parent().ok_or_else(|| config::err(path))?;
    Ok(resolve(parent)?.join(path.file_name().ok_or_else(|| config::err(path))?))
}
pub fn fingerprint(path: &Path) -> Result<String> {
    let mut digest = Sha256::new();
    walk_hash(path, path, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}
fn walk_hash(base: &Path, path: &Path, digest: &mut Sha256) -> Result<()> {
    let entries = list(path)?;
    let (dirs, files): (Vec<_>, Vec<_>) = entries.into_iter().partition(|p| p.is_dir());
    for p in dirs.iter().chain(files.iter()) {
        let relative = p.strip_prefix(base).map_err(|_| config::err(p))?;
        digest.update(relative.to_str().ok_or_else(|| config::err(p))?.as_bytes());
        if p.is_symlink() {
            digest.update(b"link:");
            digest.update(
                fs::read_link(p)
                    .map_err(|_| config::err(p))?
                    .to_str()
                    .ok_or_else(|| config::err(p))?
                    .as_bytes(),
            );
        } else if p.is_file() {
            let mut f = fs::File::open(p).map_err(|_| config::err(p))?;
            let mut buf = [0; 65536];
            loop {
                let n = f.read(&mut buf).map_err(|_| config::err(p))?;
                if n == 0 {
                    break;
                }
                digest.update(&buf[..n]);
            }
        }
    }
    for p in dirs {
        if !p.is_symlink() {
            walk_hash(base, &p, digest)?;
        }
    }
    Ok(())
}
fn unique_dir(parent: &Path, prefix: &str) -> Result<PathBuf> {
    config::private_dir(parent)?;
    loop {
        let p = config::temporary(parent, prefix);
        mutations::before(Mutation::Directory {
            path: &p,
            mode: 0o700,
        })?;
        match fs::create_dir(&p) {
            Ok(()) => {
                fs::set_permissions(&p, fs::Permissions::from_mode(0o700))
                    .map_err(|_| config::err(&p))?;
                mutations::after(&p)?;
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(config::err(&p)),
        }
    }
}
fn copy_contents(source: &Path, dest: &Path, ancestors: &mut BTreeSet<PathBuf>) -> Result<()> {
    let real = fs::canonicalize(source).map_err(|_| config::err(source))?;
    if !ancestors.insert(real.clone()) {
        return Err(format!("Skill resource cycle: {}", source.display()));
    }
    let result = (|| {
        for from in list(source)? {
            let to = dest.join(from.file_name().unwrap());
            let meta = fs::metadata(&from).map_err(|_| config::err(&from))?;
            if meta.is_dir() {
                mutations::before(Mutation::Directory {
                    path: &to,
                    mode: meta.permissions().mode() & 0o7777,
                })?;
                fs::create_dir(&to).map_err(|_| config::err(&to))?;
                copy_contents(&from, &to, ancestors)?;
                fs::set_permissions(&to, meta.permissions()).map_err(|_| config::err(&to))?;
            } else if meta.is_file() {
                let bytes = fs::read(&from).map_err(|_| config::err(&from))?;
                mutations::before(Mutation::File {
                    path: &to,
                    bytes: &bytes,
                    mode: meta.permissions().mode() & 0o7777,
                })?;
                fs::copy(&from, &to).map_err(|_| config::err(&from))?;
            } else {
                return Err(format!("Unsupported skill resource: {}", from.display()));
            }
        }
        Ok(())
    })();
    ancestors.remove(&real);
    result
}
fn prepared(source: &Path, canonical: &Path, prefix: &str) -> Result<PathBuf> {
    let tmp = unique_dir(canonical, prefix)?;
    if let Err(e) = copy_contents(source, &tmp, &mut BTreeSet::new()) {
        if mutations::before(Mutation::Remove { path: &tmp }).is_ok() {
            let _ = fs::remove_dir_all(&tmp);
        }
        return Err(e);
    }
    Ok(tmp)
}
// Nombre `skill-<uuid4 hex>` como el Python: aleatorio, visible, modo 0700.
fn backup_dir(parent: &Path) -> Result<PathBuf> {
    config::private_dir(parent)?;
    loop {
        let mut bytes = [0u8; 16];
        fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|_| config::err(parent))?;
        bytes[6] = bytes[6] & 0x0f | 0x40;
        bytes[8] = bytes[8] & 0x3f | 0x80;
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let p = parent.join(format!("skill-{hex}"));
        mutations::before(Mutation::Directory {
            path: &p,
            mode: 0o700,
        })?;
        match fs::create_dir(&p) {
            Ok(()) => {
                fs::set_permissions(&p, fs::Permissions::from_mode(0o700))
                    .map_err(|_| config::err(&p))?;
                mutations::after(&p)?;
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(config::err(&p)),
        }
    }
}
fn backup(home: &Path, path: &Path) -> Result<PathBuf> {
    let root = backup_dir(&config::state_dir(home).join("backups"))?;
    let backup = root.join(path.file_name().ok_or_else(|| config::err(path))?);
    mutations::before(Mutation::Move {
        source: path,
        target: &backup,
    })?;
    fs::rename(path, &backup).map_err(|_| config::err(path))?;
    Ok(backup)
}
fn rename_new(source: &Path, dest: &Path) -> Result<()> {
    mutations::before(Mutation::Move {
        source,
        target: dest,
    })?;
    let from = fs::File::open(source.parent().ok_or_else(|| config::err(source))?)
        .map_err(|_| config::err(source))?;
    let to = fs::File::open(dest.parent().ok_or_else(|| config::err(dest))?)
        .map_err(|_| config::err(dest))?;
    #[cfg(not(target_os = "macos"))]
    let renamed = nix::fcntl::renameat2(
        from,
        source.file_name().ok_or_else(|| config::err(source))?,
        to,
        dest.file_name().ok_or_else(|| config::err(dest))?,
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    );
    #[cfg(target_os = "macos")]
    let renamed = rustix::fs::renameat_with(
        from,
        source.file_name().ok_or_else(|| config::err(source))?,
        to,
        dest.file_name().ok_or_else(|| config::err(dest))?,
        rustix::fs::RenameFlags::NOREPLACE,
    );
    renamed.map_err(|_| config::err(dest))
}
fn replace_tree(home: &Path, path: &Path, tmp: &Path) -> Result<()> {
    let displaced = backup(home, path)?;
    if path.exists() || path.is_symlink() {
        return Err(format!("Skill destination changed: {}", path.display()));
    }
    if let Err(_e) = rename_new(tmp, path) {
        if !path.exists() && !path.is_symlink() {
            let _ = rename_new(&displaced, path);
        }
        return Err(config::err(path));
    }
    Ok(())
}
pub fn sync_skills(home: &Path) -> Result<Vec<PathBuf>> {
    let candidates = roots(home)?;
    let canonical = &candidates[0];
    mutations::before(Mutation::Directory {
        path: canonical,
        mode: 0o777,
    })?;
    fs::create_dir_all(canonical).map_err(|_| config::err(canonical))?;
    mutations::after(canonical)?;
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    for root in &candidates {
        if seen.insert(resolve(root)?) {
            roots.push(root.clone());
        }
    }
    let snapshot_path = config::state_dir(home).join("skills.json");
    let previous = config::read_config(&snapshot_path)?;
    let mut proposals = BTreeMap::<String, (PathBuf, String, String)>::new();
    if previous.as_object().is_some_and(|v| !v.is_empty()) {
        for root in &roots[1..] {
            if !root.exists() {
                continue;
            }
            for candidate in list(root)? {
                if !valid(&candidate) {
                    continue;
                }
                let name = name(&candidate)?;
                let target = canonical.join(name);
                if !target.exists() || resolve(&candidate)? == resolve(&target)? {
                    continue;
                }
                let current = fingerprint(&target)?;
                let incoming = fingerprint(&candidate)?;
                if incoming == current {
                    continue;
                }
                if previous[name] != current {
                    return Err(format!(
                        "Conflicting canonical and native skill edits: {name}"
                    ));
                }
                if proposals.get(name).is_some_and(|p| p.1 != incoming) {
                    return Err(format!("Conflicting native skill edits: {name}"));
                }
                proposals.insert(name.into(), (candidate, incoming, current));
            }
        }
    }
    let mut changed = Vec::new();
    for (name, (source, incoming, current)) in proposals {
        let target = canonical.join(&name);
        let tmp = prepared(&source, canonical, "adopt")?;
        let result = (|| {
            if fingerprint(&source)? != incoming || fingerprint(&target)? != current {
                return Err(format!("Skill changed during synchronization: {name}"));
            }
            replace_tree(home, &target, &tmp)?;
            Ok(())
        })();
        if tmp.exists() && mutations::before(Mutation::Remove { path: &tmp }).is_ok() {
            let _ = fs::remove_dir_all(&tmp);
        }
        result?;
        changed.push(target);
    }
    for skill in list(canonical)? {
        if skill.is_symlink() && skill.exists() {
            let resolved = resolve(&skill)?;
            let inside = roots[1..]
                .iter()
                .filter(|r| r.exists())
                .any(|r| resolve(r).is_ok_and(|r| resolved.starts_with(r)));
            if inside {
                let before = fingerprint(&skill)?;
                let tmp = prepared(&skill, canonical, "materialized")?;
                if fingerprint(&skill)? != before {
                    if mutations::before(Mutation::Remove { path: &tmp }).is_ok() {
                        let _ = fs::remove_dir_all(tmp);
                    }
                    return Err(format!(
                        "Skill changed during synchronization: {}",
                        name(&skill)?
                    ));
                }
                let result = replace_tree(home, &skill, &tmp);
                if tmp.exists() && mutations::before(Mutation::Remove { path: &tmp }).is_ok() {
                    let _ = fs::remove_dir_all(tmp);
                }
                result?;
                changed.push(skill);
            }
        }
    }
    for root in &roots[1..] {
        if !root.exists() {
            continue;
        }
        for skill in list(root)? {
            if !valid(&skill) {
                continue;
            }
            let dest = canonical.join(name(&skill)?);
            if !dest.exists() {
                if dest.is_symlink() {
                    return Err(format!(
                        "Skill destination already exists: {}",
                        dest.display()
                    ));
                }
                let before = fingerprint(&skill)?;
                let tmp = prepared(&skill, canonical, "import")?;
                let result = (|| {
                    if fingerprint(&skill)? != before {
                        return Err(format!(
                            "Skill changed during synchronization: {}",
                            name(&skill)?
                        ));
                    }
                    if dest.exists() || dest.is_symlink() {
                        return Err(format!("Skill destination changed: {}", dest.display()));
                    }
                    rename_new(&tmp, &dest)
                })();
                if tmp.exists() && mutations::before(Mutation::Remove { path: &tmp }).is_ok() {
                    let _ = fs::remove_dir_all(tmp);
                }
                result?;
                changed.push(dest);
            }
        }
    }
    let skills = list(canonical)?
        .into_iter()
        .filter(|p| valid(p))
        .collect::<Vec<_>>();
    for root in &roots[1..] {
        mutations::before(Mutation::Directory {
            path: root,
            mode: 0o777,
        })?;
        fs::create_dir_all(root).map_err(|_| config::err(root))?;
        mutations::after(root)?;
        for stale in list(root)? {
            if stale.is_symlink()
                && !stale.exists()
                && fs::read_link(&stale)
                    .ok()
                    .is_some_and(|p| p.parent() == Some(canonical))
            {
                mutations::before(Mutation::Remove { path: &stale })?;
                fs::remove_file(&stale).map_err(|_| config::err(&stale))?;
                changed.push(stale);
            }
        }
        for skill in &skills {
            let dest = root.join(name(skill)?);
            if dest.is_symlink() && resolve(&dest)? == resolve(skill)? {
                continue;
            }
            let displaced = if dest.exists() || dest.is_symlink() {
                Some(backup(home, &dest)?)
            } else {
                None
            };
            mutations::before(Mutation::Link {
                path: &dest,
                target: skill,
            })?;
            if symlink(skill, &dest).is_err() {
                if let Some(displaced) = displaced
                    && !dest.exists()
                    && !dest.is_symlink()
                {
                    let _ = rename_new(&displaced, &dest);
                }
                return Err(config::err(&dest));
            }
            changed.push(dest);
        }
    }
    let mut snapshot = json!({});
    for skill in skills {
        snapshot[name(&skill)?] = json!(fingerprint(&skill)?);
    }
    if config::read_config(&snapshot_path)? != snapshot {
        config::save_json(&snapshot_path, &snapshot)?;
    }
    Ok(changed)
}
