#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "m5-bundle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn invoke(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn bundle_has_exact_executable_plist_and_valid_512_png_icns() {
    let d = Dir::new();
    let binary = d.0.join("own binary");
    fs::write(&binary, b"owned fixture executable").unwrap();
    let out = d.0.join("output");
    let result = invoke(&[
        "mac-bundle",
        "--target",
        "aarch64-apple-darwin",
        "--binary",
        binary.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let contents = out.join("ComandOS.app/Contents");
    assert_eq!(
        fs::read(contents.join("MacOS/comandos-app-mac")).unwrap(),
        b"owned fixture executable"
    );
    assert_ne!(
        fs::metadata(contents.join("MacOS/comandos-app-mac"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    let plist = fs::read_to_string(contents.join("Info.plist")).unwrap();
    for text in [
        "ai.0xai.comandos",
        "ComandOS",
        "comandos-app-mac",
        "12.0",
        "NSHighResolutionCapable",
        "<true/>",
        "AppIcon.icns",
    ] {
        assert!(plist.contains(text), "{text}");
    }
    assert!(plist.contains("<string>0.1.0</string>"));
    let icon = fs::read(contents.join("Resources/AppIcon.icns")).unwrap();
    let png = include_bytes!("../../dash/icon-512.png");
    assert_eq!(&icon[..4], b"icns");
    assert_eq!(
        u32::from_be_bytes(icon[4..8].try_into().unwrap()) as usize,
        icon.len()
    );
    assert_eq!(&icon[8..12], b"ic09");
    assert_eq!(
        u32::from_be_bytes(icon[12..16].try_into().unwrap()) as usize,
        png.len() + 8
    );
    assert_eq!(&icon[16..], png);
    assert_eq!(&png[16..24], &[0, 0, 2, 0, 0, 0, 2, 0]);
    assert!(
        !invoke(&[
            "mac-bundle",
            "--target",
            "aarch64-apple-darwin",
            "--binary",
            binary.to_str().unwrap(),
            "--out",
            out.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(
        fs::read(contents.join("MacOS/comandos-app-mac")).unwrap(),
        b"owned fixture executable"
    );
}
#[test]
fn invalid_bundle_arguments_and_dry_run_never_publish() {
    let d = Dir::new();
    let binary = d.0.join("binary");
    fs::write(&binary, b"owned").unwrap();
    let out = d.0.join("out");
    for tail in [
        vec!["--target", "linux"],
        vec![
            "--target",
            "x86_64-apple-darwin",
            "--adhoc",
            "--sign",
            "own identity",
        ],
    ] {
        let mut args = vec![
            "mac-bundle",
            "--binary",
            binary.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ];
        args.extend(tail);
        assert!(!invoke(&args).status.success());
        assert!(!out.exists());
    }
    assert!(
        invoke(&[
            "mac-bundle",
            "--target",
            "x86_64-apple-darwin",
            "--binary",
            binary.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--dry-run"
        ])
        .status
        .success()
    );
    assert!(!out.exists());
}
#[test]
fn sync_dry_run_requires_absolute_remote_staging_and_never_runs_tools() {
    let d = Dir::new();
    let marker = d.0.join("marker");
    let fake = d.0.join("ssh");
    fs::write(
        &fake,
        format!("#!/bin/sh\nprintf ran > '{}'\nexit 99\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "mac-sync",
            "--host",
            "own-host",
            "--remote-dir",
            "/private/owned staging",
            "--remote-cargo",
            "/private/owned tools/cargo",
            "--dry-run",
        ])
        .env("PATH", &d.0)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("git bundle create"));
    assert!(text.contains("/private/owned staging"));
    assert!(text.contains("comandos-app-mac"));
    assert!(!marker.exists());
    assert!(
        !invoke(&[
            "mac-sync",
            "--host",
            "-bad",
            "--remote-dir",
            "relative",
            "--dry-run"
        ])
        .status
        .success()
    );
    assert!(!marker.exists());
}
#[test]
fn signing_failure_never_publishes_and_preserves_exact_identity_arguments() {
    let d = Dir::new();
    let binary = d.0.join("binary");
    fs::write(&binary, b"owned").unwrap();
    let options = xtask::mac::BundleOptions {
        target: "x86_64-apple-darwin".into(),
        binary,
        out: d.0.join("out"),
        sign: Some("literal signing identity".into()),
        dry_run: false,
    };
    let mut count = 0;
    let error = xtask::mac::bundle_with(&options, true, |program, args| {
        count += 1;
        assert_eq!(program, "codesign");
        assert_eq!(
            &args[..5],
            [
                "--force",
                "--sign",
                "literal signing identity",
                "--options",
                "runtime"
            ]
        );
        assert!(
            PathBuf::from(&args[5])
                .join("Contents/Info.plist")
                .is_file()
        );
        Ok(7)
    })
    .unwrap_err();
    assert!(error.to_string().contains("7"));
    assert_eq!(count, 1);
    assert!(!options.out.join("ComandOS.app").exists());
    assert_eq!(fs::read_dir(&options.out).unwrap().count(), 0);
}
#[test]
fn sync_builds_real_owned_git_bundle_and_only_mocked_remote_tools() {
    let d = Dir::new();
    let root = d.0.join("repo");
    fs::create_dir(&root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "owned"],
        vec![
            "-c",
            "user.name=owned fixture",
            "-c",
            "user.email=owned@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "owned",
        ],
    ] {
        if args[0] == "add" {
            fs::write(root.join("owned"), b"owned source").unwrap();
        }
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let options = xtask::mac::SyncOptions {
        host: "own-host".into(),
        remote_dir: "/private/own staging".into(),
        remote_cargo: "/private/own-toolchain/cargo".into(),
        remote_cargo_home: "/private/own-cache".into(),
        dry_run: false,
    };
    let mut calls = Vec::new();
    let mut local_bundle = None;
    xtask::mac::sync_with(&options, &root, |program, args| {
        calls.push((program.to_string(), args.to_vec()));
        if program == "git" {
            Ok(Command::new(program)
                .args(args)
                .status()?
                .code()
                .unwrap_or(1))
        } else {
            if program == "scp" {
                let bundle = PathBuf::from(&args[2]);
                assert!(fs::metadata(&bundle).unwrap().len() > 0);
                assert!(
                    Command::new("git")
                        .arg("-C")
                        .arg(&root)
                        .args(["bundle", "verify"])
                        .arg(&bundle)
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
                local_bundle = Some(bundle);
            }
            Ok(0)
        }
    })
    .unwrap();
    assert_eq!(
        calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
        ["git", "ssh", "scp", "ssh"]
    );
    assert!(
        calls[3]
            .1
            .last()
            .unwrap()
            .contains("CARGO_HOME='/private/own-cache'")
    );
    assert!(
        calls[3]
            .1
            .last()
            .unwrap()
            .contains("RUSTC='/private/own-toolchain/rustc'")
    );
    assert!(calls[3].1.last().unwrap().contains("TMPDIR="));
    assert!(!local_bundle.unwrap().exists());
}
