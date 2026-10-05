//! `claude_trust` contra `lib/claude_trust.py`: dos HOME temporales sembrados
//! igual, el Python marca uno y el port el otro; los `.claude.json` (y los
//! candados) quedan byte a byte iguales y la respuesta coincide. Solo archivos
//! bajo los HOME temporales; ningún `~/.claude.json` real.
use comandos_runtime::{claude_trust, launch_command};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(comandos_runtime::fresh_id(tag).unwrap());
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn s(&self) -> String {
        self.0.display().to_string()
    }
    fn put(&self, relative: &str, text: &str) {
        let p = self.0.join(relative);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `claude_trust.inherit_cwd_trust(...)` en el HOME `home` (como texto JSON),
/// o `"error"` si lanza; `None` sin `python3`.
fn python(home: &Home, cwd: &str, source: Option<&str>, dest: &str) -> Option<String> {
    let code = format!(
        "import sys, json; sys.path.insert(0, {lib:?}); import claude_trust\n\
         try:\n    print(json.dumps(claude_trust.inherit_cwd_trust({cwd:?}, source_config_dir={src}, dest_config_dir={dest:?}, home={home:?})))\n\
         except Exception as e:\n    print('error')",
        lib = repo().join("lib").display().to_string(),
        src = source.map_or("None".to_owned(), |s| format!("{s:?}")),
        home = home.s(),
    );
    let out = Command::new("python3")
        .arg("-c")
        .arg(code)
        .env("HOME", &home.0)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Árbol de archivos (ruta relativa, contenido, modo) bajo `home`, con la raíz
/// del HOME como `~`.
fn tree(home: &Home) -> Vec<(String, String, u32)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String, u32)>, home: &str) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            let rel = path.strip_prefix(root).unwrap().display().to_string();
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
            if path.is_dir() {
                out.push((rel, String::new(), mode));
                walk(&path, root, out, home);
            } else {
                let text = fs::read_to_string(&path).unwrap().replace(home, "~");
                out.push((rel, text, mode));
            }
        }
    }
    let mut out = Vec::new();
    walk(&home.0, &home.0, &mut out, &home.s());
    out
}

type Seed = fn(&Home);

fn cases() -> Vec<(&'static str, Seed, &'static str, Option<&'static str>)> {
    vec![
        // Origen con la carpeta exacta: el HOME ya tiene la marca (no se
        // reescribe) y el destino se crea.
        (
            "exacta",
            |h| {
                let cwd = h.0.join("codebase/p");
                fs::create_dir_all(&cwd).unwrap();
                h.put(
                    ".claude.json",
                    &format!(
                        "{{\"x\": 1, \"projects\": {{\"{}\": {{\"hasTrustDialogAccepted\": true, \"o\": \"é\"}}}}}}",
                        cwd.display()
                    ),
                );
            },
            "codebase/p",
            None,
        ),
        // Por ancestro hasta la raíz git: el HOME gana la entrada exacta
        // (se reescribe con `indent=2`) y el destino roto queda intacto.
        (
            "ancestro",
            |h| {
                fs::create_dir_all(h.0.join("codebase/r/.git")).unwrap();
                fs::create_dir_all(h.0.join("codebase/r/sub")).unwrap();
                h.put(
                    ".claude.json",
                    &format!(
                        "{{\"projects\": {{\"{}\": {{\"hasTrustDialogAccepted\": 1}}}}, \"n\": 1.50, \"u\": \"ñ\"}}",
                        h.0.join("codebase/r").display()
                    ),
                );
                h.put(".claude-accounts/rel/.claude.json", "{roto");
            },
            "codebase/r/sub",
            None,
        ),
        // Sin aceptación en el origen: nada.
        (
            "sin",
            |h| {
                fs::create_dir_all(h.0.join("codebase/q")).unwrap();
                h.put(".claude.json", "{\"projects\": {}}");
            },
            "codebase/q",
            None,
        ),
        // `projects` que no es objeto: la excepción del `any()` (False).
        (
            "proyectos-lista",
            |h| {
                fs::create_dir_all(h.0.join("codebase/q")).unwrap();
                h.put(".claude.json", "{\"projects\": [1]}");
            },
            "codebase/q",
            None,
        ),
        // Origen en un directorio de cuenta.
        (
            "origen-cuenta",
            |h| {
                let cwd = h.0.join("codebase/s");
                fs::create_dir_all(&cwd).unwrap();
                h.put(
                    ".claude-accounts/otra/.claude.json",
                    &format!(
                        "{{\"projects\": {{\"{}\": {{\"hasTrustDialogAccepted\": true}}}}}}",
                        cwd.display()
                    ),
                );
                h.put(".claude.json", "[]");
            },
            "codebase/s",
            Some(".claude-accounts/otra"),
        ),
        // La carpeta es el HOME: nunca.
        ("home", |h| h.put(".claude.json", "{}"), "", None),
    ]
}

#[test]
fn inherit_cwd_trust_matches_python() {
    for (name, seed, cwd, source) in cases() {
        let (ours, theirs) = (Home::new("trust-a"), Home::new("trust-b"));
        seed(&ours);
        seed(&theirs);
        let at = |h: &Home| {
            if cwd.is_empty() {
                h.s()
            } else {
                h.0.join(cwd).display().to_string()
            }
        };
        let src = |h: &Home| source.map(|s| h.0.join(s).display().to_string());
        let dest = |h: &Home| h.0.join(".claude-accounts/rel").display().to_string();
        let Some(py) = python(
            &theirs,
            &at(&theirs),
            src(&theirs).as_deref(),
            &dest(&theirs),
        ) else {
            eprintln!("python3 no está instalado: se salta");
            return;
        };
        let got = claude_trust::inherit_cwd_trust(
            &at(&ours),
            src(&ours).as_deref(),
            &dest(&ours),
            &ours.s(),
        )
        .unwrap();
        let rust = match got {
            Some(v) => json!(v).to_string(),
            None => "error".into(),
        };
        assert_eq!(rust, py, "{name}");
        assert_eq!(tree(&ours), tree(&theirs), "{name}");
    }
}

#[test]
fn switch_only_between_distinct_claude_accounts() {
    let h = Home::new("trust-switch");
    let cwd = h.0.join("codebase/p");
    fs::create_dir_all(&cwd).unwrap();
    h.put(
        ".claude.json",
        &format!(
            "{{\"projects\": {{\"{}\": {{\"hasTrustDialogAccepted\": true}}}}}}",
            cwd.display()
        ),
    );
    let registry = json!({"harnesses": {"claude": {"accountsRoot": "~/.claude-accounts"}}});
    let cwd = cwd.display().to_string();
    let call = |from: &str, to: &str, harness: &str| {
        launch_command::inherit_trust_for_switch(&registry, &h.s(), &cwd, from, to, harness)
    };
    assert_eq!(call("main", "main", "claude"), Ok(false));
    assert_eq!(call("", "main", "claude"), Ok(false));
    assert_eq!(call("main", "rel", "grok"), Ok(false));
    assert!(call("main", "rel", "codex").is_err(), "Codex: 2f-2/T1");
    assert_eq!(call("main", "rel", "claude"), Ok(true));
    let dest = h.0.join(".claude-accounts/rel/.claude.json");
    let text = fs::read_to_string(dest).unwrap();
    assert!(text.contains("hasTrustDialogAccepted"), "{text}");
    assert_eq!(
        launch_command::trust_note(&cwd, "", "rel"),
        format!("trust heredado main -> rel en {cwd}")
    );
}

/// Desviación deliberada del Python (que deja el `.claude.json` con el modo
/// de su temporal, 0666 menos la umask): heredar la confianza conserva el
/// modo original, así un `~/.claude.json` 0600 sigue 0600 (y el de la cuenta
/// destino también). Un archivo nuevo nace como en el Python.
#[test]
fn stamping_keeps_the_original_mode() {
    let h = Home::new("trust-mode");
    let cwd = h.0.join("codebase/r/sub");
    fs::create_dir_all(h.0.join("codebase/r/.git")).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    h.put(
        ".claude.json",
        &format!(
            "{{\"projects\": {{\"{}\": {{\"hasTrustDialogAccepted\": true}}}}}}",
            h.0.join("codebase/r").display()
        ),
    );
    h.put(".claude-accounts/rel/.claude.json", "{\"x\": 1}");
    // Un temporal viejo y abierto a todos no debe contagiar su modo.
    h.put(".claude.json.tmp", "resto");
    let set = |rel: &str, mode: u32| {
        fs::set_permissions(h.0.join(rel), fs::Permissions::from_mode(mode)).unwrap();
    };
    set(".claude.json", 0o600);
    set(".claude.json.tmp", 0o666);
    set(".claude-accounts/rel/.claude.json", 0o640);
    let dest = h.0.join(".claude-accounts/rel").display().to_string();
    let got = claude_trust::inherit_cwd_trust(&cwd.display().to_string(), None, &dest, &h.s());
    assert_eq!(got, Ok(Some(true)));
    let mode = |rel: &str| fs::metadata(h.0.join(rel)).unwrap().permissions().mode() & 0o7777;
    let home_json = fs::read_to_string(h.0.join(".claude.json")).unwrap();
    assert!(
        home_json.contains(&cwd.display().to_string()),
        "{home_json}"
    );
    assert_eq!(mode(".claude.json"), 0o600);
    assert_eq!(mode(".claude-accounts/rel/.claude.json"), 0o640);
    assert!(!h.0.join(".claude.json.tmp").exists());
}
