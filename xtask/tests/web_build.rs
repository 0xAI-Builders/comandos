// xtask/tests/web_build.rs
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use xtask::web_build::{
    self, BindgenTarget, Manifest, Options, Tools, WasmCrate, content_hash, render_boot,
};

#[test]
fn boot_is_generated_short_and_logic_free() {
    let js = render_boot("./comandos_web.js", "./comandos_web_bg.wasm");
    assert!(js.lines().count() <= 30, "{js}");
    assert!(js.contains("import init, { boot } from \"./comandos_web.js\""));
    assert!(js.contains(
        "await init({ module_or_path: new URL(\"./comandos_web_bg.wasm\", import.meta.url) })"
    ));
    assert!(
        !js.contains("fetch(\"/"),
        "el cargador no habla con el servidor: lo hace el WASM"
    );
}

#[test]
fn hash_is_stable_and_12_hex() {
    let h = content_hash(&[b"a".as_slice(), b"b".as_slice()]);
    assert_eq!(h.len(), 12);
    assert_eq!(h, content_hash(&[b"a".as_slice(), b"b".as_slice()]));
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn manifest_round_trips() {
    let mut m = Manifest::default();
    m.files
        .insert("boot.js".into(), "0123456789ab/boot.js".into());
    let back: Manifest = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.files["boot.js"], "0123456789ab/boot.js");
}

#[test]
fn known_crates_have_their_budgets_and_targets() {
    let k = web_build::known_crates();
    let by = |n: &str| k.iter().find(|c| c.name == n).cloned().unwrap();
    assert_eq!(by("comandos-web").budget_gzip, 600 * 1024);
    assert_eq!(by("comandos-web").target, BindgenTarget::Web);
    assert_eq!(by("comandos-term-web").budget_gzip, 250 * 1024);
    assert_eq!(by("comandos-term-web").target, BindgenTarget::Web);
    assert_eq!(by("comandos-web-sw").budget_gzip, 64 * 1024);
    assert_eq!(by("comandos-web-sw").target, BindgenTarget::NoModules);
    assert_eq!(by("comandos-web").lib, "comandos_web");
}

#[test]
fn bindgen_argv_uses_the_crate_target() {
    let sw = WasmCrate::new("comandos-web-sw", BindgenTarget::NoModules, 64 * 1024);
    let argv = web_build::bindgen_args(&sw, Path::new("/t/a.wasm"), Path::new("/o"));
    let argv: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        [
            "--target",
            "no-modules",
            "--no-typescript",
            "--out-dir",
            "/o",
            "--out-name",
            "comandos_web_sw",
            "/t/a.wasm"
        ]
    );
    let web = WasmCrate::new("comandos-web", BindgenTarget::Web, 1);
    let argv = web_build::bindgen_args(&web, Path::new("/t/a.wasm"), Path::new("/o"));
    assert_eq!(
        argv.get(1).map(|s| s.to_string_lossy().into_owned()),
        Some("web".into())
    );
}

#[test]
fn parse_args_selects_crates_and_budget() {
    let a = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };
    let p = web_build::parse_args(&a(&[])).unwrap();
    assert_eq!(p.0, None);
    assert!(!p.1);
    let p = web_build::parse_args(&a(&["--crate", "comandos-web", "--check-budget"])).unwrap();
    assert_eq!(p.0.as_deref(), Some("comandos-web"));
    assert!(p.1);
    let p = web_build::parse_args(&a(&["--crate", "all"])).unwrap();
    assert_eq!(p.0, None);
    assert!(web_build::parse_args(&a(&["--crate", "otro"])).is_err());
    assert!(web_build::parse_args(&a(&["--crate"])).is_err());
    assert!(web_build::parse_args(&a(&["--nada"])).is_err());
}

#[test]
fn gzip_budget_is_enforced_only_when_asked() {
    let c = WasmCrate::new("x", BindgenTarget::Web, 10);
    // Bytes pseudoaleatorios: gzip no los encoge por debajo de 10.
    let data: Vec<u8> = (0u32..4096)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let len = web_build::gzip_len(&data).unwrap();
    assert!(len > 10);
    assert!(web_build::check_budget(&c, len, false).is_ok());
    let err = web_build::check_budget(&c, len, true).unwrap_err();
    assert!(err.contains("presupuesto"), "{err}");
    let roomy = WasmCrate::new("x", BindgenTarget::Web, len);
    assert!(web_build::check_budget(&roomy, len, true).is_ok());
}

/// Directorio de compilación resuelto como lo resuelve cargo (R6).
fn cargo_metadata_target_dir(env: Option<&str>, cwd: &Path) -> PathBuf {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args([
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
        "--offline",
    ])
    .arg("--manifest-path")
    .arg(workspace().join("Cargo.toml"))
    .current_dir(cwd);
    match env {
        Some(v) => cmd.env("CARGO_TARGET_DIR", v),
        None => cmd.env_remove("CARGO_TARGET_DIR"),
    };
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    PathBuf::from(v["target_directory"].as_str().unwrap())
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn out_dir_matches_cargo_metadata() {
    let ws = workspace();
    let cwd = std::env::temp_dir();
    for env in [None, Some("/tmp/cmd-t4-abs-target"), Some("rel-target")] {
        let expect = cargo_metadata_target_dir(env, &cwd).join("web");
        let got = comandos_core::web_assets::out_dir_from(env.map(Path::new), &cwd, &ws);
        assert_eq!(got, expect, "CARGO_TARGET_DIR={env:?}");
    }
    // La función pública lee el entorno real de esta prueba.
    let env = std::env::var("CARGO_TARGET_DIR").ok();
    let here = std::env::current_dir().unwrap();
    assert_eq!(
        web_build::out_dir().unwrap(),
        cargo_metadata_target_dir(env.as_deref(), &here).join("web")
    );
}

#[test]
fn root_profile_release_wasm_is_declared() {
    let toml = fs::read_to_string(workspace().join("Cargo.toml")).unwrap();
    let block = toml.split("[profile.release-wasm]").nth(1).unwrap();
    let block = block.split("\n[").next().unwrap();
    for line in [
        "inherits = \"release\"",
        "opt-level = \"z\"",
        "lto = true",
        "codegen-units = 1",
        "panic = \"abort\"",
        "strip = \"symbols\"",
    ] {
        assert!(block.contains(line), "falta {line}");
    }
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cmd-t4-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// Compila de verdad un cdylib mínimo (fixture de prueba, no producto) con la
/// cadena completa: cargo → wasm-bindgen → wasm-opt → hash → manifest.
#[test]
fn builds_a_fixture_crate_end_to_end() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/wasm-fixture");
    let target = web_build::target_dir().unwrap().join("xtask-wasm-fixture");
    let out = scratch("out");
    let spec = WasmCrate::new("fixture-web", BindgenTarget::Web, 600 * 1024);
    let opts = Options {
        manifest_path: fixture.join("Cargo.toml"),
        target_dir: target.clone(),
        out: out.clone(),
        crates: vec![spec.clone()],
        check_budget: true,
        tools: Tools::from_env(),
    };
    let m = web_build::build(&opts).unwrap();
    let wasm_rel = m.path("fixture_web_bg.wasm").unwrap().to_string();
    let (hash, name) = wasm_rel.split_once('/').unwrap();
    assert_eq!(hash.len(), 12);
    assert_eq!(name, "fixture_web_bg.wasm");
    assert_eq!(
        m.path("fixture_web.js"),
        Some(format!("{hash}/fixture_web.js").as_str())
    );
    assert_eq!(
        m.path("fixture_web_boot.js"),
        Some(format!("{hash}/boot.js").as_str())
    );
    let wasm = fs::read(out.join(&wasm_rel)).unwrap();
    assert_eq!(wasm.get(..4), Some(b"\0asm".as_slice()));
    let boot = fs::read_to_string(out.join(hash).join("boot.js")).unwrap();
    assert_eq!(
        boot,
        render_boot("./fixture_web.js", "./fixture_web_bg.wasm")
    );
    // El manifiesto en disco es el devuelto.
    let disk: Manifest =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(disk, m);
    // wasm-opt -Oz encoge la salida de wasm-bindgen.
    let raw = target.join("wasm32-unknown-unknown/release-wasm/fixture_web.wasm");
    assert!(wasm.len() < fs::metadata(raw).unwrap().len() as usize);

    // Determinista: otra pasada da el mismo hash y no deja directorios sobrantes.
    fs::create_dir_all(out.join("deadbeef0000")).unwrap(); // hash viejo
    fs::create_dir_all(out.join("no-es-hash")).unwrap(); // ajeno: se respeta
    let again = web_build::build(&opts).unwrap();
    assert_eq!(again, m);
    assert!(!out.join("deadbeef0000").exists());
    assert!(out.join("no-es-hash").exists());

    // Presupuesto mínimo: falla con --check-budget y pasa sin él.
    let tight = Options {
        crates: vec![WasmCrate::new("fixture-web", BindgenTarget::Web, 16)],
        ..opts.clone()
    };
    let err = web_build::build(&tight).unwrap_err();
    assert!(err.contains("presupuesto"), "{err}");
    let lax = Options {
        check_budget: false,
        ..tight
    };
    assert!(web_build::build(&lax).is_ok());
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn building_one_crate_keeps_the_other_entries() {
    let out = scratch("merge");
    let mut old = Manifest::default();
    old.files.insert(
        "comandos_web_sw.js".into(),
        "0123456789ab/comandos_web_sw.js".into(),
    );
    old.files.insert(
        "comandos_web.js".into(),
        "aaaaaaaaaaaa/comandos_web.js".into(),
    );
    fs::create_dir_all(out.join("0123456789ab")).unwrap();
    fs::create_dir_all(out.join("aaaaaaaaaaaa")).unwrap();
    let mut new = Manifest::default();
    new.files.insert(
        "comandos_web.js".into(),
        "bbbbbbbbbbbb/comandos_web.js".into(),
    );
    fs::create_dir_all(out.join("bbbbbbbbbbbb")).unwrap();
    let merged = web_build::merge_and_prune(&out, old, new).unwrap();
    assert_eq!(
        merged.path("comandos_web.js"),
        Some("bbbbbbbbbbbb/comandos_web.js")
    );
    assert_eq!(
        merged.path("comandos_web_sw.js"),
        Some("0123456789ab/comandos_web_sw.js")
    );
    assert!(out.join("0123456789ab").exists());
    assert!(out.join("bbbbbbbbbbbb").exists());
    assert!(!out.join("aaaaaaaaaaaa").exists());
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn missing_tools_fail_with_the_install_command() {
    let tools = Tools {
        cargo: PathBuf::from(env!("CARGO")),
        wasm_bindgen: PathBuf::from("/nonexistent/wasm-bindgen"),
        wasm_opt: PathBuf::from("/nonexistent/wasm-opt"),
    };
    let err = tools.check().unwrap_err();
    assert!(
        err.contains("cargo install --locked wasm-bindgen-cli --version =0.2.129"),
        "{err}"
    );
}
