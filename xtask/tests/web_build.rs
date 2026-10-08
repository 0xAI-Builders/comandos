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
    assert_eq!(by("comandos-term-web").budget_gzip, 252 * 1024);
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

/// Directorio temporal bajo el de compilación del fixture: mismo sistema de
/// archivos que `<target>/web-build/`, como `<target>/web` en producción, para que
/// el `rename` final de `web-build` no cruce dispositivos (`/tmp` puede ser tmpfs).
fn scratch_in_target(tag: &str) -> PathBuf {
    let d = web_build::target_dir()
        .unwrap()
        .join("xtask-wasm-fixture/tests")
        .join(format!("cmd-t4-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// Herramientas wasm o `None` con un aviso: las pruebas que compilan de verdad se
/// saltan (no fallan) en una máquina sin `wasm-bindgen 0.2.129`, `wasm-opt 116` o
/// el target `wasm32-unknown-unknown`.
fn wasm_tools_or_skip(test: &str) -> Option<Tools> {
    let tools = Tools::from_env();
    if let Err(e) = tools.check() {
        eprintln!("SKIP {test}: {e}");
        return None;
    }
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let sysroot = Command::new(rustc)
        .args(["--print", "sysroot"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let has_target = sysroot.is_some_and(|s| {
        Path::new(&s)
            .join("lib/rustlib/wasm32-unknown-unknown")
            .is_dir()
    });
    if !has_target {
        eprintln!("SKIP {test}: falta el target (rustup target add wasm32-unknown-unknown)");
        return None;
    }
    Some(tools)
}

fn fixture_opts(out: &Path, tools: Tools) -> Options {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/wasm-fixture");
    Options {
        manifest_path: fixture.join("Cargo.toml"),
        target_dir: web_build::target_dir().unwrap().join("xtask-wasm-fixture"),
        out: out.to_path_buf(),
        crates: vec![WasmCrate::new(
            "fixture-web",
            BindgenTarget::Web,
            600 * 1024,
        )],
        check_budget: true,
        tools,
    }
}

/// Lista recursiva (ruta relativa, bytes) de un árbol.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            if p.is_dir() {
                out.push((rel, Vec::new()));
                todo.push(p);
            } else {
                out.push((rel, fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// Compila de verdad un cdylib mínimo (fixture de prueba, no producto) con la
/// cadena completa: cargo → wasm-bindgen → wasm-opt → hash → manifest.
#[test]
fn builds_a_fixture_crate_end_to_end() {
    let Some(tools) = wasm_tools_or_skip("builds_a_fixture_crate_end_to_end") else {
        return;
    };
    let root = scratch_in_target("e2e");
    let out = root.join("web");
    let opts = fixture_opts(&out, tools);
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
    // El snippet de `inline_js` va plano junto al cargador, con clave de
    // manifiesto prefijada por el crate, y el módulo JS lo importa por ese nombre.
    let (key, snippet) = m
        .files
        .iter()
        .find(|(k, _)| k.starts_with("fixture_web_snippets-fixture-web-"))
        .unwrap_or_else(|| panic!("sin snippet en {m:?}"));
    assert!(key.ends_with("-inline0.js"), "{key}");
    let flat = snippet.strip_prefix(&format!("{hash}/")).unwrap();
    assert_eq!(key, &format!("fixture_web_{flat}"));
    assert!(
        fs::read_to_string(out.join(snippet))
            .unwrap()
            .contains("export function twice")
    );
    let module = fs::read_to_string(out.join(hash).join("fixture_web.js")).unwrap();
    assert!(module.contains(&format!("'./{flat}'")), "{module}");
    assert!(!module.contains("./snippets/"));
    let wasm = fs::read(out.join(&wasm_rel)).unwrap();
    assert_eq!(wasm.get(..4), Some(b"\0asm".as_slice()));
    let boot = fs::read_to_string(out.join(hash).join("boot.js")).unwrap();
    assert_eq!(
        boot,
        render_boot("./fixture_web.js", "./fixture_web_bg.wasm")
    );
    // El manifiesto en disco es el devuelto; la salida solo tiene lo que referencia.
    let disk: Manifest =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(disk, m);
    let mut top: Vec<String> = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    top.sort();
    assert_eq!(
        top,
        [
            web_build::OUTPUT_OWNER_FILE.to_string(),
            hash.to_string(),
            "manifest.json".to_string()
        ]
    );
    // wasm-opt -Oz encoge la salida de wasm-bindgen.
    let raw = opts
        .target_dir
        .join("wasm32-unknown-unknown/release-wasm/fixture_web.wasm");
    assert!(wasm.len() < fs::metadata(raw).unwrap().len() as usize);

    // Determinista: otra pasada da el mismo hash; la salida es solo de web-build, así
    // que lo no referenciado (hash viejo o ajeno) desaparece.
    fs::create_dir_all(out.join("deadbeef0000")).unwrap();
    fs::create_dir_all(out.join("no-es-hash")).unwrap();
    let foreign = snapshot(&out);
    assert!(web_build::build(&opts).is_err());
    assert_eq!(
        snapshot(&out),
        foreign,
        "foreign content cannot be discarded"
    );
    fs::remove_dir_all(out.join("deadbeef0000")).unwrap();
    fs::remove_dir_all(out.join("no-es-hash")).unwrap();
    let again = web_build::build(&opts).unwrap();
    assert_eq!(again, m);
    assert!(!out.join("deadbeef0000").exists());
    assert!(!out.join("no-es-hash").exists());

    // Presupuesto mínimo: falla con --check-budget sin tocar la salida, y pasa sin él.
    let before = snapshot(&out);
    let tight = Options {
        crates: vec![WasmCrate::new("fixture-web", BindgenTarget::Web, 16)],
        ..opts.clone()
    };
    let err = web_build::build(&tight).unwrap_err();
    assert!(err.contains("presupuesto"), "{err}");
    assert_eq!(snapshot(&out), before, "un build fallido no toca la salida");
    let lax = Options {
        check_budget: false,
        ..tight
    };
    assert!(web_build::build(&lax).is_ok());
    // Ningún temporal en la salida ni restos en la zona de trabajo.
    let work = opts.target_dir.join("web-build");
    let stray: Vec<_> = fs::read_dir(&work)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("out.") || n.starts_with("old."))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_failed_first_build_leaves_no_output_dir() {
    let Some(tools) = wasm_tools_or_skip("a_failed_first_build_leaves_no_output_dir") else {
        return;
    };
    let root = scratch_in_target("fail");
    let out = root.join("web");
    let mut opts = fixture_opts(&out, tools);
    opts.crates = vec![WasmCrate::new("no-existe", BindgenTarget::Web, 600 * 1024)];
    let err = web_build::build(&opts).unwrap_err();
    assert!(err.contains("no-existe"), "{err}");
    assert!(!out.exists(), "sin <target>/web tras un build fallido");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn assembling_one_crate_keeps_the_other_entries_and_swaps_atomically() {
    let root = scratch("merge");
    let out = root.join("web");
    let next = root.join("next");
    // Salida vieja: sw vivo, web viejo, y algo ajeno.
    for (dir, file) in [
        ("0123456789ab", "comandos_web_sw.js"),
        ("aaaaaaaaaaaa", "comandos_web.js"),
    ] {
        fs::create_dir_all(out.join(dir)).unwrap();
        fs::write(out.join(dir).join(file), dir).unwrap();
    }
    let mut old = Manifest::default();
    old.files.insert(
        "comandos_web_sw.js".into(),
        "0123456789ab/comandos_web_sw.js".into(),
    );
    old.files.insert(
        "comandos_web.js".into(),
        "aaaaaaaaaaaa/comandos_web.js".into(),
    );
    web_build::assemble(&root.join("absent"), &out, Manifest::default(), old.clone()).unwrap();
    // Lo recién compilado ya está en `next`.
    fs::create_dir_all(next.join("bbbbbbbbbbbb")).unwrap();
    fs::write(next.join("bbbbbbbbbbbb/comandos_web.js"), "nuevo").unwrap();
    let mut fresh = Manifest::default();
    fresh.files.insert(
        "comandos_web.js".into(),
        "bbbbbbbbbbbb/comandos_web.js".into(),
    );
    let merged = web_build::assemble(&out, &next, old, fresh).unwrap();
    assert_eq!(
        merged.path("comandos_web.js"),
        Some("bbbbbbbbbbbb/comandos_web.js")
    );
    assert_eq!(
        merged.path("comandos_web_sw.js"),
        Some("0123456789ab/comandos_web_sw.js")
    );
    // La salida vieja sigue intacta hasta el cambio.
    assert!(out.join("aaaaaaaaaaaa").exists());
    web_build::swap_into_place(&next, &out, &root.join("trash")).unwrap();
    assert!(!next.exists());
    assert!(!root.join("trash").exists());
    assert_eq!(
        fs::read_to_string(out.join("0123456789ab/comandos_web_sw.js")).unwrap(),
        "0123456789ab"
    );
    assert_eq!(
        fs::read_to_string(out.join("bbbbbbbbbbbb/comandos_web.js")).unwrap(),
        "nuevo"
    );
    assert!(!out.join("aaaaaaaaaaaa").exists());
    assert!(!out.join("no-es-hash").exists());
    let disk: Manifest =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(disk, merged);
    // Sin salida previa, el cambio es un rename simple.
    let next2 = root.join("next2");
    fs::create_dir_all(&next2).unwrap();
    fs::create_dir(next2.join("cccccccccccc")).unwrap();
    fs::write(next2.join("cccccccccccc/comandos_web.js"), "fresh").unwrap();
    let mut fresh2 = Manifest::default();
    fresh2.files.insert(
        "comandos_web.js".into(),
        "cccccccccccc/comandos_web.js".into(),
    );
    web_build::assemble(&root.join("absent"), &next2, Manifest::default(), fresh2).unwrap();
    let fresh_out = root.join("web2");
    web_build::swap_into_place(&next2, &fresh_out, &root.join("trash2")).unwrap();
    assert!(fresh_out.is_dir());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn wasm_opt_version_must_be_exactly_116() {
    assert!(web_build::wasm_opt_version_ok(
        "wasm-opt version 116 (version_116)"
    ));
    assert!(web_build::wasm_opt_version_ok("wasm-opt version 116"));
    assert!(!web_build::wasm_opt_version_ok(
        "wasm-opt version 1160 (version_1160)"
    ));
    assert!(!web_build::wasm_opt_version_ok(
        "wasm-opt version 117 (version_116)"
    ));
    assert!(!web_build::wasm_opt_version_ok("wasm-opt 116"));
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

#[test]
fn snippets_are_flattened_next_to_the_loader_and_imports_rewritten() {
    // `inline_js` de wasm-bindgen sale como `snippets/<crate>-<hash>/inlineN.js`;
    // el frente sirve `/web/<hash>/<archivo>` (un segmento), así que van planos.
    let js = "import { api_fetch } from './snippets/comandos-web-dom-90199b/inline0.js';\nimport * as x from \"./snippets/otro-1/inline1.js\";\nlet y = 1;\n";
    let snippets = vec![
        (
            "snippets/comandos-web-dom-90199b/inline0.js".to_string(),
            b"export function api_fetch(){}".to_vec(),
        ),
        (
            "snippets/otro-1/inline1.js".to_string(),
            b"export const z = 1;".to_vec(),
        ),
    ];
    let (out, files) = web_build::flatten_snippets(js, snippets).unwrap();
    assert_eq!(
        out,
        "import { api_fetch } from './snippets-comandos-web-dom-90199b-inline0.js';\nimport * as x from \"./snippets-otro-1-inline1.js\";\nlet y = 1;\n"
    );
    assert_eq!(
        files.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        [
            "snippets-comandos-web-dom-90199b-inline0.js",
            "snippets-otro-1-inline1.js"
        ]
    );
    assert_eq!(files[0].1, b"export function api_fetch(){}");
}

#[test]
fn a_snippet_nobody_imports_is_not_shipped() {
    // wasm-bindgen escribe el fragmento aunque LTO haya quitado sus usos
    // (arranque vacío): no se copia.
    let (out, files) =
        web_build::flatten_snippets("let y = 1;", vec![("snippets/a/inline0.js".into(), vec![])])
            .unwrap();
    assert_eq!(out, "let y = 1;");
    assert!(files.is_empty());
}

#[test]
fn an_import_of_a_snippet_that_was_not_found_is_an_error() {
    let r = web_build::flatten_snippets("import { f } from './snippets/b/inline0.js';", vec![]);
    assert!(r.unwrap_err().contains("./snippets/"));
}

#[test]
fn two_snippets_that_flatten_to_the_same_name_are_an_error() {
    let js = "import './snippets/a-b/c.js';\nimport './snippets/a/b-c.js';\n";
    let r = web_build::flatten_snippets(
        js,
        vec![
            ("snippets/a-b/c.js".into(), vec![]),
            ("snippets/a/b-c.js".into(), vec![]),
        ],
    );
    assert!(r.unwrap_err().contains("snippets-a-b-c.js"));
}

#[test]
fn separate_output_preserves_default_and_refuses_source_or_roots() {
    let args = vec!["--out".into(), "/private/artifacts".into()];
    assert_eq!(
        web_build::parse_args(&args).unwrap().2,
        Some(PathBuf::from("/private/artifacts"))
    );
    assert_eq!(web_build::parse_args(&[]).unwrap().2, None);
    assert!(web_build::validate_output(Path::new("relative/artifacts")).is_err());
    assert!(web_build::validate_output(Path::new("/")).is_err());
    assert!(web_build::validate_output(&PathBuf::from(env!("CARGO_MANIFEST_DIR"))).is_err());
    let private = scratch("output-policy");
    let source = private.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("canary.rs"), "source unchanged").unwrap();
    let link = private.join("artifact-link");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    assert!(web_build::validate_output(&link).is_err());
    assert!(web_build::validate_output(&source).is_err());
    assert_eq!(
        fs::read_to_string(source.join("canary.rs")).unwrap(),
        "source unchanged"
    );
    fs::remove_dir_all(private).unwrap();
}

#[test]
fn content_loader_waits_for_native_dependency_and_refuses_failed_transport() {
    let root = scratch("content-loader");
    let loader = root.join("content_boot.js");
    fs::write(
        &loader,
        web_build::render_content_boot("./comandos_web.js", "./comandos_web_bg.wasm"),
    )
    .unwrap();
    let proof = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/content_loader.cjs");
    let output = Command::new("node")
        .args(["--experimental-vm-modules"])
        .arg(proof)
        .arg(loader)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
