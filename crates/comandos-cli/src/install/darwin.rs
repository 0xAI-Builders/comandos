//! User-owned macOS installation. Preview never writes or invokes platform tools.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
pub const AGENT_NAME: &str = "com.0xai.cc-dash.plist";
const MAX_FILE: u64 = 512 << 20;
static NEXT: AtomicUsize = AtomicUsize::new(0);
type Launch<'a> = Option<&'a mut dyn FnMut(&str, &Path) -> Result<(), String>>;
fn error(path: &Path, e: impl std::fmt::Display) -> String {
    format!("{}: {e}", path.display())
}
fn check(path: &Path) -> Result<(), String> {
    super::release::check_app_parents(path)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn home_check(home: &Path) -> Result<(), String> {
    check(&home.join("placeholder"))?;
    if !home.symlink_metadata().is_ok_and(|m| m.is_dir()) {
        return Err(error(home, "existing regular HOME directory required"));
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<(), String> {
    check(&path.join("placeholder"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|e| error(path, e))?;
    check(&path.join("placeholder"))
}
fn open_regular(path: &Path) -> Result<fs::File, String> {
    check(path)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY)
        .open(path)
        .map_err(|e| error(path, e))?;
    if !file.metadata().map_err(|e| error(path, e))?.is_file() {
        return Err(error(path, "regular file required"));
    }
    Ok(file)
}
fn stamp(m: &fs::Metadata) -> (u64, u64, u64, i64, i64) {
    (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec())
}
fn read(path: &Path, private: bool) -> Result<Option<Vec<u8>>, String> {
    check(path)?;
    match path.symlink_metadata() {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(path, e)),
        Ok(m) if !m.is_file() => return Err(error(path, "regular file required")),
        _ => {}
    }
    let mut file = open_regular(path)?;
    let before = file.metadata().map_err(|e| error(path, e))?;
    if before.len() > 1 << 20 || private && before.permissions().mode() & 0o777 != 0o600 {
        return Err(error(path, "bounded private 0600 record required"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((1 << 20) + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| error(path, e))?;
    let after = file.metadata().map_err(|e| error(path, e))?;
    let named = path.symlink_metadata().map_err(|e| error(path, e))?;
    if bytes.len() > 1 << 20
        || stamp(&before) != stamp(&after)
        || (named.dev(), named.ino()) != (before.dev(), before.ino())
    {
        return Err(error(path, "file changed during read"));
    }
    Ok(Some(bytes))
}
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    check(path)?;
    let parent = path.parent().ok_or("missing parent")?;
    private_dir(parent)?;
    let tmp = parent.join(format!(
        ".comandos-m5-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| error(&tmp, e))?;
    let _guard = TempFile(tmp.clone());
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| error(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| error(path, e))?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| error(parent, e))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    check(path)?;
    let parent = path.parent().ok_or("missing parent")?;
    let tmp = parent.join(format!(
        ".comandos-backup-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| error(&tmp, e))?;
    let _guard = TempFile(tmp.clone());
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| error(&tmp, e))?;
    fs::hard_link(&tmp, path).map_err(|e| error(path, e))?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| error(parent, e))
}
struct Lock(fs::File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
fn rollback_dir(home: &Path) -> PathBuf {
    home.join(".local/share/comandos/rollback")
}
fn lock(home: &Path) -> Result<Lock, String> {
    let dir = rollback_dir(home);
    private_dir(&dir)?;
    let path = dir.join("darwin-install.lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY)
        .open(&path)
        .map_err(|e| error(&path, e))?;
    let meta = file.metadata().map_err(|e| error(&path, e))?;
    if !meta.is_file() || meta.permissions().mode() & 0o777 != 0o600 {
        return Err(error(&path, "private regular lock required"));
    }
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Lock(file)),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(error(&path, e)),
        }
    }
}
pub fn agent_plist(home: &Path) -> Result<String, String> {
    let text = home.to_str().ok_or("HOME must be UTF-8 for plist")?;
    let h = xml(text);
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.0xai.cc-dash</string>
  <key>ProgramArguments</key><array>
    <string>{h}/.local/share/comandos/bin/comandos</string><string>dash</string><string>--no-open</string>
  </array>
  <key>EnvironmentVariables</key><dict>
    <!-- launchd no hereda el PATH del shell: sin Homebrew aquí, cc-dash no
         encuentra tmux y /state (y toda acción tmux) muere con conexión vacía. -->
    <key>PATH</key><string>{h}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict></plist>
"#
    ))
}
fn agent_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents").join(AGENT_NAME)
}
fn agent_state(home: &Path) -> PathBuf {
    rollback_dir(home).join(format!("{AGENT_NAME}.state"))
}
fn original(home: &Path) -> PathBuf {
    rollback_dir(home).join(format!("{AGENT_NAME}.orig"))
}
struct AgentState {
    original: Option<String>,
    allowed: Vec<String>,
}
struct AgentRollback {
    current: Option<Vec<u8>>,
    original: Option<Vec<u8>>,
}
fn state(home: &Path) -> Result<Option<AgentState>, String> {
    let Some(raw) = read(&agent_state(home), true)? else {
        return Ok(None);
    };
    let v: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if v.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("unknown Darwin rollback record".into());
    }
    let original = match v.get("original") {
        Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        _ => return Err("invalid original hash".into()),
    };
    let allowed = v
        .get("allowed")
        .and_then(Value::as_array)
        .ok_or("invalid installed hashes")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(String::from)
                .ok_or_else(|| "invalid installed hash".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if allowed.len() > 2 || allowed.is_empty() {
        return Err("invalid installed hash count".into());
    }
    Ok(Some(AgentState { original, allowed }))
}
fn validate_agent(home: &Path, current: &Option<Vec<u8>>, s: &AgentState) -> Result<(), String> {
    let orig = read(&original(home), true)?;
    if orig.as_deref().map(digest) != s.original {
        return Err("original LaunchAgent backup changed or missing".into());
    }
    let hash = current.as_deref().map(digest);
    if hash != s.original && !hash.as_ref().is_some_and(|h| s.allowed.contains(h)) {
        return Err("LaunchAgent modified outside installer; refusing overwrite".into());
    }
    Ok(())
}
fn publish(path: &Path, bytes: &Option<Vec<u8>>) -> Result<(), String> {
    if let Some(bytes) = bytes {
        atomic(path, bytes)
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(error(path, e)),
        }
    }
}
fn change_agent(
    path: &Path,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    mut launch: Launch<'_>,
) -> Result<(), String> {
    if read(path, false)? != before {
        return Err("LaunchAgent changed before publication".into());
    }
    let Some(tool) = launch.as_mut() else {
        return publish(path, &after);
    };
    // Legacy launchctl unload reads the plist to find the registered label.
    // Keep it present until the service has stopped, including rollback to absence.
    if before.is_some() {
        tool("unload", path)
            .map_err(|e| format!("launchctl unload failed; previous file unchanged: {e}"))?;
    }
    if let Err(e) = publish(path, &after) {
        // Publication can fail after rename (for example, while syncing the parent).
        // Restore bytes before attempting to restart the previous service.
        publish(path, &before).map_err(|restore| format!("{e}; restore failed: {restore}"))?;
        if before.is_some()
            && let Err(reload) = tool("load", path)
        {
            return Err(format!(
                "{e}; previous file restored; service reload failed: {reload}"
            ));
        }
        return Err(e);
    }
    if after.is_some()
        && let Err(e) = tool("load", path)
    {
        // A failed load may still have registered a service. Attempt cleanup while
        // its plist remains readable, and retain any failure in the returned error.
        let cleanup = tool("unload", path).err();
        publish(path, &before).map_err(|restore| format!("{e}; restore failed: {restore}"))?;
        let reload = if before.is_some() {
            tool("load", path).err()
        } else {
            None
        };
        let mut message = format!("launchctl load failed; previous file restored: {e}");
        if let Some(cleanup) = cleanup {
            message.push_str(&format!("; failed service unload: {cleanup}"));
        }
        if let Some(reload) = reload {
            message.push_str(&format!("; previous service reload failed: {reload}"));
        }
        return Err(message);
    }
    Ok(())
}
fn launchctl(action: &str, path: &Path) -> Result<(), String> {
    let status = Command::new("launchctl")
        .arg(action)
        .arg(path)
        .status()
        .map_err(|e| error(path, e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("launchctl {action}: {status}"))
    }
}
pub fn agent(home: &Path, dry: bool, no_launchctl: bool) -> Result<(), String> {
    if !dry && !no_launchctl && !cfg!(target_os = "macos") {
        return Err("--darwin-agent activation requires Darwin; --no-launchctl writes a private fixture only".into());
    }
    if !dry && !no_launchctl {
        let exe = home.join(".local/share/comandos/bin/comandos");
        if !fs::metadata(&exe).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0) {
            return Err(error(&exe, "stage the CLI before activating LaunchAgent"));
        }
    }
    let mut tool = launchctl;
    agent_with(
        home,
        dry,
        if no_launchctl || dry {
            None
        } else {
            Some(&mut tool)
        },
    )
}
/// Injected platform boundary: tests never execute launchctl.
pub fn agent_with(home: &Path, dry: bool, launch: Launch<'_>) -> Result<(), String> {
    home_check(home)?;
    let path = agent_path(home);
    check(&path)?;
    let payload = agent_plist(home)?.into_bytes();
    if dry {
        let current = read(&path, false)?;
        let prior = state(home)?;
        if let Some(s) = &prior {
            validate_agent(home, &current, s)?;
        } else if original(home).symlink_metadata().is_ok() {
            return Err("unowned original LaunchAgent backup exists".into());
        }
        println!(
            "dry-run: LaunchAgent {}; backup {}",
            path.display(),
            original(home).display()
        );
        return Ok(());
    }
    let _lock = lock(home)?;
    let current = read(&path, false)?;
    let prior = state(home)?;
    let original_hash = if let Some(s) = &prior {
        validate_agent(home, &current, s)?;
        s.original.clone()
    } else {
        if original(home).symlink_metadata().is_ok() {
            return Err("original backup exists without record".into());
        }
        if let Some(bytes) = &current {
            write_new(&original(home), bytes)?;
        }
        current.as_deref().map(digest)
    };
    let mut allowed = vec![digest(&payload)];
    if let Some(hash) = current.as_deref().map(digest)
        && !allowed.contains(&hash)
    {
        allowed.push(hash);
    }
    atomic(
        &agent_state(home),
        &serde_json::to_vec(&json!({"version":1,"original":original_hash,"allowed":allowed}))
            .map_err(|e| e.to_string())?,
    )?;
    change_agent(&path, current, Some(payload), launch)?;
    println!("LaunchAgent {}", path.display());
    Ok(())
}
pub fn rollback_agent(home: &Path, dry: bool, no_launchctl: bool) -> Result<(), String> {
    if !dry && !no_launchctl && !cfg!(target_os = "macos") {
        return Err("LaunchAgent activation requires Darwin; --no-launchctl is available for private fixtures".into());
    }
    let mut tool = launchctl;
    rollback_agent_with(home, dry, if no_launchctl { None } else { Some(&mut tool) })
}
/// Injected platform boundary: tests never execute launchctl.
pub fn rollback_agent_with(home: &Path, dry: bool, launch: Launch<'_>) -> Result<(), String> {
    home_check(home)?;
    let inspect = || -> Result<AgentRollback, String> {
        let s = state(home)?.ok_or("no Darwin LaunchAgent rollback")?;
        let current = read(&agent_path(home), false)?;
        validate_agent(home, &current, &s)?;
        Ok(AgentRollback {
            current,
            original: read(&original(home), true)?,
        })
    };
    let _ = inspect()?;
    if dry {
        println!("dry-run: restore {}", agent_path(home).display());
        return Ok(());
    }
    let _lock = lock(home)?;
    let AgentRollback {
        current: before,
        original: after,
    } = inspect()?;
    change_agent(&agent_path(home), before, after, launch)
}

struct Tree {
    entries: Vec<(PathBuf, bool)>,
    hash: String,
}
fn tree(root: &Path) -> Result<Tree, String> {
    check(&root.join("placeholder"))?;
    if !root.symlink_metadata().is_ok_and(|m| m.is_dir()) {
        return Err(error(root, "regular app directory required"));
    }
    let mut pending = vec![PathBuf::new()];
    let mut entries = Vec::new();
    while let Some(rel) = pending.pop() {
        let dir = root.join(&rel);
        check(&dir.join("placeholder"))?;
        for entry in fs::read_dir(&dir).map_err(|e| error(&dir, e))? {
            let entry = entry.map_err(|e| error(&dir, e))?;
            let meta = entry
                .path()
                .symlink_metadata()
                .map_err(|e| error(&entry.path(), e))?;
            let rel = rel.join(entry.file_name());
            if meta.is_dir() {
                pending.push(rel.clone());
            } else if !meta.is_file() {
                return Err(error(
                    &entry.path(),
                    "app accepts regular files and directories only",
                ));
            }
            entries.push((rel, meta.is_dir()));
            if entries.len() > 4096 {
                return Err("app has too many entries".into());
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    for (rel, dir) in &entries {
        let path = rel.as_os_str().as_bytes();
        hash.update((path.len() as u64).to_be_bytes());
        hash.update(path);
        hash.update([u8::from(*dir)]);
        if !dir {
            let abs = root.join(rel);
            let mut file = open_regular(&abs)?;
            let before = file.metadata().map_err(|e| error(&abs, e))?;
            hash.update((before.permissions().mode() & 0o777).to_be_bytes());
            hash.update(before.len().to_be_bytes());
            let mut n = 0u64;
            loop {
                let read = file.read(&mut buffer).map_err(|e| error(&abs, e))?;
                if read == 0 {
                    break;
                }
                n += read as u64;
                total += read as u64;
                if total > MAX_FILE {
                    return Err("app exceeds 512MiB".into());
                }
                hash.update(&buffer[..read]);
            }
            if n != before.len()
                || stamp(&before) != stamp(&file.metadata().map_err(|e| error(&abs, e))?)
            {
                return Err("app file changed during inspection".into());
            }
        }
    }
    Ok(Tree {
        entries,
        hash: format!("{:x}", hash.finalize()),
    })
}
fn validate_bundle(path: &Path) -> Result<Tree, String> {
    let tree = tree(path)?;
    let exe = path.join("Contents/MacOS/comandos-app-mac");
    let file = open_regular(&exe)?;
    if file
        .metadata()
        .map_err(|e| error(&exe, e))?
        .permissions()
        .mode()
        & 0o111
        == 0
    {
        return Err("bundle executable is not executable".into());
    }
    if read(&path.join("Contents/Info.plist"), false)?.is_none() {
        return Err("bundle Info.plist absent".into());
    }
    Ok(tree)
}
struct TempTree(PathBuf);
impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn app_path(home: &Path) -> PathBuf {
    home.join("Applications/ComandOS.app")
}
fn previous(home: &Path) -> PathBuf {
    home.join("Applications/ComandOS.app.previous")
}
fn app_record(home: &Path) -> PathBuf {
    rollback_dir(home).join("ComandOS.app.state")
}
fn app_prior(home: &Path) -> Result<Option<Tree>, String> {
    let dest = app_path(home);
    match dest.symlink_metadata() {
        // The previous app may use the legacy executable name. Its backup is
        // governed by the complete tree hash, not the new bundle's entry point.
        Ok(_) => tree(&dest).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(&dest, e)),
    }
}
pub fn install_app(home: &Path, source: &Path, dry: bool) -> Result<(), String> {
    home_check(home)?;
    let incoming = validate_bundle(source)?;
    let before = app_prior(home)?;
    let dest = app_path(home);
    let old = previous(home);
    check(&dest)?;
    check(&old)?;
    if before
        .as_ref()
        .is_some_and(|prior| prior.hash == incoming.hash)
        && app_record(home).symlink_metadata().is_ok()
    {
        let _lock = if dry { None } else { Some(lock(home)?) };
        check_app_record(home)?;
        if validate_bundle(&dest)?.hash != incoming.hash {
            return Err("app changed during reinstall admission".into());
        }
        println!(
            "app {} already installed; original backup retained",
            dest.display()
        );
        return Ok(());
    }
    if old.symlink_metadata().is_ok() || app_record(home).symlink_metadata().is_ok() {
        return Err("app backup/record already exists; rollback before reinstall".into());
    }
    if dry {
        println!(
            "dry-run: copy {} to {}; backup {}",
            source.display(),
            dest.display(),
            old.display()
        );
        return Ok(());
    }
    let _lock = lock(home)?;
    let before_now = app_prior(home)?;
    if before_now.as_ref().map(|t| &t.hash) != before.as_ref().map(|t| &t.hash)
        || old.symlink_metadata().is_ok()
        || app_record(home).symlink_metadata().is_ok()
    {
        return Err("app destination changed before install".into());
    }
    let parent = home.join("Applications");
    private_dir(&parent)?;
    let tmp_path = parent.join(format!(
        ".ComandOS.app-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&tmp_path)
        .map_err(|e| error(&tmp_path, e))?;
    let tmp = TempTree(tmp_path);
    for (rel, dir) in &incoming.entries {
        let target = tmp.0.join(rel);
        if *dir {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&target)
                .map_err(|e| error(&target, e))?;
        } else {
            let path = source.join(rel);
            let mut from = open_regular(&path)?;
            let meta = from.metadata().map_err(|e| error(&path, e))?;
            let mut to = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(meta.permissions().mode() & 0o777)
                .open(&target)
                .map_err(|e| error(&target, e))?;
            let n = io::copy(&mut Read::by_ref(&mut from).take(MAX_FILE + 1), &mut to)
                .map_err(|e| error(&path, e))?;
            if n > MAX_FILE
                || n != meta.len()
                || stamp(&meta) != stamp(&from.metadata().map_err(|e| error(&path, e))?)
            {
                return Err("app changed during copy".into());
            }
            to.sync_all().map_err(|e| error(&target, e))?;
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(meta.permissions().mode() & 0o777),
            )
            .map_err(|e| error(&target, e))?;
        }
    }
    if tree(&tmp.0)?.hash != incoming.hash || validate_bundle(source)?.hash != incoming.hash {
        return Err("app content changed during staging".into());
    }
    atomic(&app_record(home),&serde_json::to_vec(&json!({"version":1,"installed":incoming.hash,"previous":before.as_ref().map(|t|&t.hash)})).map_err(|e|e.to_string())?)?;
    if before.is_some()
        && let Err(e) = fs::rename(&dest, &old)
    {
        let _ = fs::remove_file(app_record(home));
        return Err(error(&dest, e));
    }
    if let Err(e) = fs::rename(&tmp.0, &dest) {
        if before.is_some() {
            fs::rename(&old, &dest)
                .map_err(|restore| format!("{e}; original restore failed: {restore}"))?;
        }
        let _ = fs::remove_file(app_record(home));
        return Err(error(&dest, e));
    }
    println!("app {}", dest.display());
    Ok(())
}
fn check_app_record(home: &Path) -> Result<bool, String> {
    let raw = read(&app_record(home), true)?.ok_or("no app rollback record")?;
    let v: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if v.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("unknown app rollback record".into());
    }
    let installed = v
        .get("installed")
        .and_then(Value::as_str)
        .ok_or("invalid installed app hash")?;
    if validate_bundle(&app_path(home))?.hash != installed {
        return Err("installed app modified; refusing rollback".into());
    }
    match v.get("previous") {
        Some(Value::String(hash)) if tree(&previous(home))?.hash == *hash => Ok(true),
        Some(Value::Null) if previous(home).symlink_metadata().is_err() => Ok(false),
        _ => Err("previous app backup changed or missing".into()),
    }
}
pub fn rollback_app(home: &Path, dry: bool) -> Result<(), String> {
    home_check(home)?;
    let _ = check_app_record(home)?;
    if dry {
        println!("dry-run: restore app {}", app_path(home).display());
        return Ok(());
    }
    let _lock = lock(home)?;
    let has_previous = check_app_record(home)?;
    let parked = home.join("Applications").join(format!(
        ".ComandOS.rollback-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    if parked.symlink_metadata().is_ok() {
        return Err("rollback staging already exists".into());
    }
    fs::rename(app_path(home), &parked).map_err(|e| error(&parked, e))?;
    if has_previous && let Err(e) = fs::rename(previous(home), app_path(home)) {
        fs::rename(&parked, app_path(home))
            .map_err(|restore| format!("{e}; current app restore failed: {restore}"))?;
        return Err(error(&previous(home), e));
    }
    fs::remove_file(app_record(home)).map_err(|e| error(&app_record(home), e))?;
    fs::remove_dir_all(&parked).map_err(|e| error(&parked, e))?;
    Ok(())
}
