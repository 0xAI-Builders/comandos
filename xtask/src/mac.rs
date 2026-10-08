//! Apple checks, bundle packaging and explicit private remote build staging.
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

pub const TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

/// Check both desktop consumers on each target, stopping at the first failure.
pub fn check_with(mut check: impl FnMut(&[&str]) -> io::Result<i32>) -> io::Result<i32> {
    for target in TARGETS {
        let code = check(&[
            "check",
            "--offline",
            "-j2",
            "-p",
            "comandos-app-mac",
            "-p",
            "comandos-cli",
            "--target",
            target,
        ])?;
        if code != 0 {
            return Ok(code);
        }
    }
    Ok(0)
}

pub fn main(args: &[String]) -> i32 {
    if !args.is_empty() {
        eprintln!("uso: cargo xtask mac-check");
        return 2;
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    match check_with(|args| {
        Command::new(&cargo)
            .current_dir(&root)
            .args(args)
            .status()
            .map(|status| status.code().unwrap_or(1))
    }) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("mac-check: {error}");
            1
        }
    }
}

const BUNDLE_ID: &str = "ai.0xai.comandos";
const APP_NAME: &str = "ComandOS";
const EXECUTABLE: &str = "comandos-app-mac";
const MAX_BINARY: u64 = 512 << 20;
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn absolute(path: &Path) -> io::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(io::Error::other(format!(
            "absolute path without '..' required: {}",
            path.display()
        )));
    }
    for parent in path.ancestors().skip(1) {
        match parent.symlink_metadata() {
            Ok(meta) if meta.is_dir() => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            _ => {
                return Err(io::Error::other(format!(
                    "non-directory parent: {}",
                    parent.display()
                )));
            }
        }
    }
    Ok(())
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct OutputLock(fs::File);
impl Drop for OutputLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn version() -> io::Result<&'static str> {
    let section = include_str!("../../Cargo.toml")
        .split("[workspace.package]")
        .nth(1)
        .ok_or_else(|| io::Error::other("workspace.package absent"))?;
    section
        .split('[')
        .next()
        .unwrap_or("")
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("version = \"")
                .and_then(|v| v.strip_suffix('"'))
        })
        .ok_or_else(|| io::Error::other("workspace version absent"))
}
fn plist() -> io::Result<String> {
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
<key>CFBundleName</key><string>{APP_NAME}</string>
<key>CFBundleExecutable</key><string>{EXECUTABLE}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleIconFile</key><string>AppIcon.icns</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
"#,
        version = xml(version()?)
    ))
}
fn icon() -> io::Result<Vec<u8>> {
    // ic09 is 512x512. The plan's 192px source must not be mislabeled ic07 (128px).
    let png = include_bytes!("../../dash/icon-512.png");
    if png.get(..8) != Some(b"\x89PNG\r\n\x1a\n")
        || png.get(16..24) != Some(&[0, 0, 2, 0, 0, 0, 2, 0])
    {
        return Err(io::Error::other(
            "AppIcon requires the original 512x512 PNG",
        ));
    }
    let mut bytes = Vec::with_capacity(png.len() + 16);
    bytes.extend_from_slice(b"icns");
    bytes.extend_from_slice(
        &u32::try_from(png.len() + 16)
            .map_err(io::Error::other)?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(b"ic09");
    bytes.extend_from_slice(
        &u32::try_from(png.len() + 8)
            .map_err(io::Error::other)?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(png);
    Ok(bytes)
}
#[derive(Debug)]
pub struct BundleOptions {
    pub target: String,
    pub binary: PathBuf,
    pub out: PathBuf,
    pub sign: Option<String>,
    pub dry_run: bool,
}
fn parse_bundle(args: &[String]) -> io::Result<BundleOptions> {
    let (mut target, mut binary, mut out, mut sign, mut dry_run) = (None, None, None, None, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--target" if target.is_none() => target = it.next().cloned(),
            "--binary" if binary.is_none() => binary = it.next().map(PathBuf::from),
            "--out" if out.is_none() => out = it.next().map(PathBuf::from),
            "--sign" if sign.is_none() => sign = it.next().cloned(),
            "--adhoc" if sign.is_none() => sign = Some("-".into()),
            "--dry-run" if !dry_run => dry_run = true,
            _ => return Err(io::Error::other("invalid or repeated mac-bundle argument")),
        }
    }
    let target = target
        .filter(|v| TARGETS.contains(&v.as_str()))
        .ok_or_else(|| io::Error::other("Apple target required"))?;
    let binary = binary.ok_or_else(|| io::Error::other("--binary required"))?;
    let out = out.ok_or_else(|| io::Error::other("--out required"))?;
    if sign
        .as_ref()
        .is_some_and(|s| s.is_empty() || s.contains('\0'))
    {
        return Err(io::Error::other("invalid signing identity"));
    }
    Ok(BundleOptions {
        target,
        binary,
        out,
        sign,
        dry_run,
    })
}
/// The tool boundary is injected for private signing-failure/argument verification.
pub fn bundle_with(
    options: &BundleOptions,
    darwin: bool,
    mut run: impl FnMut(&str, &[String]) -> io::Result<i32>,
) -> io::Result<PathBuf> {
    absolute(&options.binary)?;
    absolute(&options.out)?;
    if !TARGETS.contains(&options.target.as_str()) {
        return Err(io::Error::other("Apple target required"));
    }
    let mut binary = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY)
        .open(&options.binary)?;
    let stamp = binary.metadata()?;
    if !stamp.is_file() || stamp.len() > MAX_BINARY {
        return Err(io::Error::other("bounded regular executable required"));
    }
    let dest = options.out.join("ComandOS.app");
    if dest.symlink_metadata().is_ok() {
        return Err(io::Error::other(format!(
            "bundle already exists: {}",
            dest.display()
        )));
    }
    if options.out.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
        return Err(io::Error::other("output is not a directory"));
    }
    let plist = plist()?;
    let icon = icon()?;
    if options.dry_run {
        println!(
            "dry-run: bundle {} from {} for {}; signing {:?}",
            dest.display(),
            options.binary.display(),
            options.target,
            options.sign
        );
        return Ok(dest);
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&options.out)?;
    absolute(&dest)?;
    let lock = fs::File::open(&options.out)?;
    lock.lock()?;
    let _lock = OutputLock(lock);
    if dest.symlink_metadata().is_ok() {
        return Err(io::Error::other("bundle already exists"));
    }
    let temp_path = options.out.join(format!(
        ".ComandOS.app.{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::DirBuilder::new().mode(0o700).create(&temp_path)?;
    let temp = Temp(temp_path);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(temp.0.join("Contents/MacOS"))?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(temp.0.join("Contents/Resources"))?;
    fs::write(temp.0.join("Contents/Info.plist"), plist)?;
    fs::write(temp.0.join("Contents/Resources/AppIcon.icns"), icon)?;
    let exe = temp.0.join("Contents/MacOS").join(EXECUTABLE);
    let mut target = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(&exe)?;
    let copied = io::copy(
        &mut std::io::Read::by_ref(&mut binary).take(MAX_BINARY + 1),
        &mut target,
    )?;
    let after = binary.metadata()?;
    let named = options.binary.symlink_metadata()?;
    if copied != stamp.len()
        || copied > MAX_BINARY
        || (
            after.dev(),
            after.ino(),
            after.len(),
            after.mtime(),
            after.mtime_nsec(),
        ) != (
            stamp.dev(),
            stamp.ino(),
            stamp.len(),
            stamp.mtime(),
            stamp.mtime_nsec(),
        )
        || (named.dev(), named.ino()) != (stamp.dev(), stamp.ino())
    {
        return Err(io::Error::other("binary changed while bundling"));
    }
    target.flush()?;
    target.sync_all()?;
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755))?;
    if let Some(identity) = &options.sign {
        if darwin {
            let args = vec![
                "--force".into(),
                "--sign".into(),
                identity.clone(),
                "--options".into(),
                "runtime".into(),
                temp.0.display().to_string(),
            ];
            let code = run("codesign", &args)?;
            if code != 0 {
                return Err(io::Error::other(format!("codesign exited {code}")));
            }
        } else {
            eprintln!("mac-bundle: signing requires Darwin; bundle remains unsigned");
        }
    }
    if dest.symlink_metadata().is_ok() {
        return Err(io::Error::other("bundle appeared before publication"));
    }
    fs::rename(&temp.0, &dest)?;
    Ok(dest)
}
fn tool(program: &str, args: &[String]) -> io::Result<i32> {
    Command::new(program)
        .args(args)
        .status()
        .map(|s| s.code().unwrap_or(1))
}
pub fn bundle_main(args: &[String]) -> i32 {
    match parse_bundle(args).and_then(|o| bundle_with(&o, cfg!(target_os = "macos"), tool)) {
        Ok(dest) => {
            println!("{}", dest.display());
            0
        }
        Err(e) => {
            eprintln!("mac-bundle: {e}");
            1
        }
    }
}

#[derive(Debug)]
pub struct SyncOptions {
    pub host: String,
    pub remote_dir: String,
    pub remote_cargo: String,
    pub remote_cargo_home: String,
    pub dry_run: bool,
}
fn parse_sync(args: &[String]) -> io::Result<SyncOptions> {
    let (mut host, mut dir, mut cargo, mut cargo_home, mut dry) = (None, None, None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--host" if host.is_none() => host = it.next().cloned(),
            "--remote-dir" if dir.is_none() => dir = it.next().cloned(),
            "--remote-cargo" if cargo.is_none() => cargo = it.next().cloned(),
            "--remote-cargo-home" if cargo_home.is_none() => cargo_home = it.next().cloned(),
            "--dry-run" if !dry => dry = true,
            _ => return Err(io::Error::other("invalid or repeated mac-sync argument")),
        }
    }
    let host = host.ok_or_else(|| io::Error::other("--host required"))?;
    if host.is_empty()
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b))
    {
        return Err(io::Error::other("invalid SSH host"));
    }
    let remote_dir =
        dir.ok_or_else(|| io::Error::other("--remote-dir ABS required (private staging)"))?;
    let remote_cargo =
        cargo.ok_or_else(|| io::Error::other("--remote-cargo ABS required (private toolchain)"))?;
    let remote_cargo_home = cargo_home.unwrap_or_else(|| format!("{remote_dir}/.cargo"));
    for path in [&remote_dir, &remote_cargo, &remote_cargo_home] {
        if !Path::new(path).is_absolute()
            || path.contains(['\0', '\n', '\r'])
            || Path::new(path)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(io::Error::other("absolute remote paths required"));
        }
    }
    Ok(SyncOptions {
        host,
        remote_dir,
        remote_cargo,
        remote_cargo_home,
        dry_run: dry,
    })
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
/// No shell on the local host. Remote paths are explicit and shell-quoted.
pub fn sync_with(
    options: &SyncOptions,
    root: &Path,
    mut run: impl FnMut(&str, &[String]) -> io::Result<i32>,
) -> io::Result<()> {
    let ticket = NEXT.fetch_add(1, Ordering::Relaxed);
    let remote = format!(
        "{}/.comandos-sync-{}-{ticket}.bundle",
        options.remote_dir,
        std::process::id()
    );
    let dir = quote(&options.remote_dir);
    let artifact = quote(&remote);
    let home = quote(&format!("{}/.home", options.remote_dir));
    let cache = quote(&options.remote_cargo_home);
    let target = quote(&format!("{}/.target", options.remote_dir));
    let toolchain = Path::new(&options.remote_cargo)
        .parent()
        .ok_or_else(|| io::Error::other("cargo has no parent"))?;
    let rustc = quote(&toolchain.join("rustc").display().to_string());
    let environment = format!(
        "export HOME={home} CARGO_HOME={cache} CARGO_TARGET_DIR={target} RUSTC={rustc} XDG_CONFIG_HOME={home}/config XDG_DATA_HOME={home}/data XDG_CACHE_HOME={home}/cache XDG_STATE_HOME={home}/state XDG_RUNTIME_DIR={home}/runtime TMPDIR={home}/tmp TMP={home}/tmp TEMP={home}/tmp; export PATH={}:$PATH;",
        quote(&toolchain.display().to_string())
    );
    let remote_prepare = format!(
        "umask 077; mkdir -p -- {dir} {home} {cache} {home}/config {home}/data {home}/cache {home}/state {home}/runtime {home}/tmp && {{ {environment} if [ ! -d {dir}/.git ]; then git init -- {dir}; fi; }}"
    );
    let remote_build = format!(
        "umask 077; {environment} git -C {dir} fetch -- {artifact} HEAD && git -C {dir} checkout --detach FETCH_HEAD && cd -- {dir} && {} build --release -p comandos-app-mac -p comandos-cli; rc=$?; rm -f -- {artifact}; exit $rc",
        quote(&options.remote_cargo)
    );
    if options.dry_run {
        println!(
            "dry-run: git bundle create <private temporary bundle> HEAD\nssh {} {}\nscp <private temporary bundle> {}:{}\nssh {} {}",
            options.host, remote_prepare, options.host, remote, options.host, remote_build
        );
        return Ok(());
    }
    let temp_path =
        std::env::temp_dir().join(format!("comandos-mac-sync-{}-{ticket}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&temp_path)?;
    let temp = Temp(temp_path);
    let bundle = temp.0.join("HEAD.bundle");
    let steps = [
        (
            "git",
            vec![
                "-C".into(),
                root.display().to_string(),
                "bundle".into(),
                "create".into(),
                bundle.display().to_string(),
                "HEAD".into(),
            ],
        ),
        (
            "ssh",
            vec![
                "-o".into(),
                "BatchMode=yes".into(),
                options.host.clone(),
                remote_prepare,
            ],
        ),
        (
            "scp",
            vec![
                "-B".into(),
                "-O".into(),
                bundle.display().to_string(),
                format!("{}:{}", options.host, quote(&remote)),
            ],
        ),
        (
            "ssh",
            vec![
                "-o".into(),
                "BatchMode=yes".into(),
                options.host.clone(),
                remote_build,
            ],
        ),
    ];
    for (program, args) in steps {
        let outcome = run(program, &args);
        if !matches!(outcome, Ok(0)) {
            let _ = run(
                "ssh",
                &[
                    "-o".into(),
                    "BatchMode=yes".into(),
                    options.host.clone(),
                    format!("rm -f -- {artifact}"),
                ],
            );
            return Err(match outcome {
                Ok(code) => io::Error::other(format!("mac-sync {program} exited {code}")),
                Err(e) => e,
            });
        }
    }
    Ok(())
}
pub fn sync_main(args: &[String]) -> i32 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    match parse_sync(args).and_then(|o| sync_with(&o, &root, tool)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("mac-sync: {e}");
            1
        }
    }
}
