//! Apple cross-check entry point; execution waits for the AppKit consumer (M3).
use std::{io, process::Command};

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
