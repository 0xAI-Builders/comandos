//! C5 fresh scoped oracles: reuse the accepted implementations without changing them.
use serde_json::json;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "c5-tools-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for p in [
            "home",
            "config",
            "data",
            "state",
            "cache",
            "runtime",
            "tmp",
            "bin",
            "dash",
            "tools/cli-commands/scraped",
        ] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(root.join(p))
                .unwrap();
        }
        Self(root)
    }
    fn python(&self, script: &Path, args: &[&Path]) -> std::process::Output {
        let mut c = Command::new("/usr/bin/python3");
        c.env_clear()
            .args(["-I"])
            .arg(script)
            .args(args)
            .env("PATH", self.0.join("bin"))
            .env("HOME", self.0.join("home"));
        for (k, v) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_RUNTIME_DIR", "runtime"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            c.env(k, self.0.join(v));
        }
        c.output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn css_private_bytes_match_original() {
    let f = Fixture::new();
    fs::write(
        f.0.join("tools/css_orphans.py"),
        include_bytes!("../../tools/css_orphans.py"),
    )
    .unwrap();
    fs::write(
        f.0.join("dash/index.html"),
        "<style>.gone,.used,#old,.café {color:#literal}</style><p class='used café'>雪</p>",
    )
    .unwrap();
    fs::write(f.0.join("dash/a.js"), "let old=1").unwrap();
    let original = f.python(&f.0.join("tools/css_orphans.py"), &[]);
    assert!(original.status.success());
    let names = xtask::css_orphans::scan(&f.0, &[]).unwrap();
    assert_eq!(
        format!("{}\n", names.join("\n")).as_bytes(),
        original.stdout
    );
    assert!(original.stderr.is_empty());
}
#[test]
fn catalogue_private_and_frozen_checkout_are_byte_identical() {
    let f = Fixture::new();
    fs::write(
        f.0.join("tools/cli-commands/build.py"),
        include_bytes!("../../tools/cli-commands/build.py"),
    )
    .unwrap();
    let old = json!({"clis":[{"id":"grok","title":"雪","groups":[],"launch":"ignored"},{"id":"codex","extra":"literal '$()","groups":[]}]});
    fs::write(
        f.0.join("config/cli-commands.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    for (id, commands) in [
        (
            "grok",
            json!([
                ["m", "model (currently X)"],
                ["t", "alias"],
                ["effort", "choose"],
                ["m", "duplicate"]
            ]),
        ),
        ("codex", json!([["cd", "directory"], ["unknown", "雪"]])),
    ] {
        fs::write(
            f.0.join(format!("tools/cli-commands/scraped/{id}.json")),
            serde_json::to_vec(&json!({"version":"fixture","commands":commands})).unwrap(),
        )
        .unwrap();
    }
    let native = xtask::cli_catalog::build_bytes(&f.0).unwrap();
    let original = f.python(&f.0.join("tools/cli-commands/build.py"), &[]);
    assert!(
        original.status.success(),
        "{}",
        String::from_utf8_lossy(&original.stderr)
    );
    assert_eq!(
        native,
        fs::read(f.0.join("config/cli-commands.json")).unwrap()
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let before = fs::read(root.join("config/cli-commands.json")).unwrap();
    assert_eq!(xtask::cli_catalog::build_bytes(root).unwrap(), before);
    assert_eq!(
        fs::read(root.join("config/cli-commands.json")).unwrap(),
        before
    );
}
#[test]
fn png_original_text_exit_and_decoded_diff_pixels_match() {
    use xtask::png_diff::{Rgba, decode, diff, write_diff_png, write_png};
    let f = Fixture::new();
    fs::write(
        f.0.join("tools/png_diff.py"),
        include_bytes!("../../tools/png_diff.py"),
    )
    .unwrap();
    let a = Rgba {
        width: 2,
        height: 2,
        pixels: [10, 20, 30, 255].repeat(4),
    };
    let mut b = a.clone();
    b.pixels[1] = 61;
    b.pixels[5] = 62;
    let ap = f.0.join("a.png");
    let bp = f.0.join("b.png");
    let py = f.0.join("original.png");
    let rust = f.0.join("native.png");
    write_png(&a, &ap).unwrap();
    write_png(&b, &bp).unwrap();
    let original = f.python(&f.0.join("tools/png_diff.py"), &[&ap, &bp, &py]);
    let stats = diff(&a, &b, 24).unwrap();
    assert_eq!(
        format!("{:.3}% de píxeles distintos\n", stats.ratio() * 100.0).as_bytes(),
        original.stdout
    );
    assert_eq!(
        original.status.code(),
        Some(if stats.ratio() * 100.0 <= 0.1 { 0 } else { 1 })
    );
    assert!(original.stderr.is_empty());
    write_diff_png(&a, &b, 24, &rust).unwrap();
    assert_eq!(
        decode(&fs::read(py).unwrap()).unwrap(),
        decode(&fs::read(rust).unwrap()).unwrap()
    );
}
