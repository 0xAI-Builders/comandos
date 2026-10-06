//! Extrae los cuerpos de producción y sustituye únicamente colaboradores de GUI/HTTP.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]
use comandos_app::proc::{ProcSpec, run};
use std::path::{Path, PathBuf};
fn body(source: &str, needle: &str) -> String {
    let start = source.find(needle).expect(needle);
    let brace = start + source[start..].find('{').unwrap();
    let mut level = 0;
    for (i, b) in source.as_bytes().iter().enumerate().skip(brace) {
        if *b == b'{' {
            level += 1;
        } else if *b == b'}' {
            level -= 1;
            if level == 0 {
                return source[start..=i]
                    .replace("pub(super) ", "")
                    .replace("pub(in crate::ui) ", "");
            }
        }
    }
    panic!("unclosed {needle}")
}
fn library(dir: &Path, name: &str) -> PathBuf {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|f| f.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "rlib")
                && p.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with(&format!("lib{name}-"))
        })
        .max_by_key(|p| p.metadata().unwrap().modified().unwrap())
        .unwrap()
}
fn dependency(deps: &Path, app: &Path, name: &str) -> PathBuf {
    let hash = app
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("libcomandos_app-")
        .unwrap();
    let fingerprints = deps.parent().unwrap().join(".fingerprint");
    let meta: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fingerprints.join(format!("comandos-app-{hash}/lib-comandos_app.json")))
            .unwrap(),
    )
    .unwrap();
    let wanted = meta["deps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d[1] == name)
        .unwrap()[3]
        .as_u64()
        .unwrap();
    for dir in std::fs::read_dir(&fingerprints).unwrap().flatten() {
        let encoded = dir.path().join(format!("lib-{name}"));
        if let Ok(text) = std::fs::read_to_string(encoded) {
            let bytes: Vec<_> = (0..8)
                .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
                .collect();
            if u64::from_le_bytes(bytes.try_into().unwrap()) == wanted {
                let folder = dir.file_name();
                let suffix = folder.to_str().unwrap().rsplit('-').next().unwrap();
                let library = deps.join(format!("lib{name}-{suffix}.rlib"));
                if library.exists() {
                    return library;
                }
            }
        }
    }
    panic!("missing matched dependency {name}")
}
pub fn execute(input: &serde_json::Value) -> serde_json::Value {
    let mut methods = body(
        include_str!("../../src/ui/app_t17_panes.rs"),
        "fn pane_card(",
    ) + &body(
        include_str!("../../src/ui/app_t17_panes.rs"),
        "fn card_button(",
    );
    let page = include_str!("../../src/ui/webview.rs");
    methods.push_str(
        &(body(page, "pub(crate) struct OwnedPage {")
            + &body(page, "impl OwnedPage {")
            + &body(page, "impl Drop for OwnedPage {")),
    );
    let source = include_str!("t17_widgets.rs").replace("// ACTUAL_METHODS", &methods);
    execute_source(&source, input)
}
pub fn execute_source(source: &str, input: &serde_json::Value) -> serde_json::Value {
    let dir = std::env::temp_dir().join(format!("t17-native-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rust = dir.join("probe.rs");
    let bin = dir.join("probe");
    std::fs::write(&rust, source).unwrap();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let app = library(&deps, "comandos_app");
    let mut args = vec![
        "--edition=2024".into(),
        rust.display().to_string().into(),
        "-o".into(),
        bin.display().to_string().into(),
        "-L".into(),
        format!("dependency={}", deps.display()).into(),
        "--extern".into(),
        format!("comandos_app={}", app.display()).into(),
        "--extern".into(),
        format!(
            "serde_json={}",
            dependency(&deps, &app, "serde_json").display()
        )
        .into(),
        "--extern".into(),
        format!("glib={}", dependency(&deps, &app, "glib").display()).into(),
    ];
    let flags = run(&ProcSpec {
        program: "pkg-config".into(),
        args: vec!["--libs-only-L".into(), "webkit2gtk-4.1".into()],
        stdin: None,
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(flags.code, Some(0));
    for flag in String::from_utf8(flags.stdout).unwrap().split_whitespace() {
        if let Some(path) = flag.strip_prefix("-L") {
            args.push("-L".into());
            args.push(format!("native={path}").into());
        }
    }
    let result = run(&ProcSpec {
        program: "rustc".into(),
        args,
        stdin: None,
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(30),
    })
    .unwrap();
    std::fs::write(dir.join("compile.log"), &result.stderr).unwrap();
    assert_eq!(
        result.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = run(&ProcSpec {
        program: bin.display().to_string(),
        args: vec![],
        stdin: Some(serde_json::to_vec(input).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    std::fs::write(dir.join("run.log"), &result.stderr).unwrap();
    std::fs::write(dir.join("result.json"), &result.stdout).unwrap();
    assert_eq!(
        result.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
