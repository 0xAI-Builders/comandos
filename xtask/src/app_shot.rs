use serde_json::Value;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppShotArgs {
    pub remote: String,
    pub manifest: PathBuf,
    pub output: PathBuf,
    pub measure: bool,
}

pub fn parse(args: &[String]) -> Result<AppShotArgs, String> {
    let mut remote = None;
    let mut manifest = None;
    let mut output = None;
    let mut measure = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--remote" => {
                remote = args.get(i + 1).cloned();
                i += 2;
            }
            "--manifest" => {
                manifest = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--output" => {
                output = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--measure" => {
                measure = true;
                i += 1;
            }
            other => return Err(format!("opción desconocida: {other}")),
        }
    }
    let parsed = AppShotArgs {
        remote: remote.ok_or("--remote requerido")?,
        manifest: manifest.ok_or("--manifest requerido")?,
        output: output.ok_or("--output requerido")?,
        measure,
    };
    if !parsed.manifest.is_absolute() || !parsed.output.is_absolute() {
        return Err("--manifest y --output deben ser rutas absolutas locales".into());
    }
    Ok(parsed)
}

pub fn run(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    run_with_probe(&parsed, &EnvProbe::from_env())
}

fn run_with_probe(parsed: &AppShotArgs, probe: &dyn RemoteProbe) -> i32 {
    if let Err(e) = validate_manifest(&parsed.manifest) {
        eprintln!(
            "app-shot: manifiesto inválido {}: {e}",
            parsed.manifest.display()
        );
        return 2;
    }
    let missing = preflight_missing(parsed, probe);
    if !missing.is_empty() {
        eprintln!(
            "app-shot: runtime remoto no disponible en {}: {}",
            parsed.remote,
            missing.join(", ")
        );
        return 2;
    }
    match probe.capture(parsed) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("app-shot: captura remota falló en {}: {e}", parsed.remote);
            2
        }
    }
}

trait RemoteProbe {
    fn check(&self, parsed: &AppShotArgs, capability: &str) -> Result<bool, String>;
    fn capture(&self, parsed: &AppShotArgs) -> Result<(), String>;
}

const REQUIRED_CAPABILITIES: &[&str] = &[
    "linux-gtk3-runtime",
    "webkitgtk-2.50",
    "isolated-display",
    "remote-capture-tool",
    "private-tmux",
    "fixture-http",
    "python-gi-vte-oracle",
    "rust-toolchain",
];

fn preflight_missing(parsed: &AppShotArgs, probe: &dyn RemoteProbe) -> Vec<String> {
    let mut missing = Vec::new();
    for capability in REQUIRED_CAPABILITIES {
        match probe.check(parsed, capability) {
            Ok(true) => {}
            Ok(false) => missing.push((*capability).to_string()),
            Err(e) => missing.push(format!("{capability} ({e})")),
        }
    }
    if parsed.remote != "macmini" {
        missing.push("supported-remote-macmini".to_string());
    }
    missing
}

struct EnvProbe {
    command: Option<OsString>,
}

impl EnvProbe {
    fn from_env() -> Self {
        Self {
            command: std::env::var_os("COMANDOS_APP_SHOT_PROBE").filter(|s| !s.is_empty()),
        }
    }

    fn configured(&self) -> Result<&OsString, String> {
        self.command
            .as_ref()
            .ok_or_else(|| "remote-probe-command".to_string())
    }
}

impl RemoteProbe for EnvProbe {
    fn check(&self, parsed: &AppShotArgs, capability: &str) -> Result<bool, String> {
        let command = self.configured()?;
        let status = Command::new(command)
            .arg("--remote")
            .arg(&parsed.remote)
            .arg("--manifest")
            .arg(&parsed.manifest)
            .arg("--check")
            .arg(capability)
            .status()
            .map_err(|e| e.to_string())?;
        Ok(status.success())
    }

    fn capture(&self, parsed: &AppShotArgs) -> Result<(), String> {
        let command = self.configured()?;
        let mut cmd = Command::new(command);
        cmd.arg("--remote")
            .arg(&parsed.remote)
            .arg("--manifest")
            .arg(&parsed.manifest)
            .arg("--output")
            .arg(&parsed.output)
            .arg("--capture");
        if parsed.measure {
            cmd.arg("--measure");
        }
        let status = cmd.status().map_err(|e| e.to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("probe salió con {status}"))
    }
}

fn validate_manifest(path: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("la raíz debe ser un objeto JSON".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, fs};

    struct FakeProbe {
        missing: Vec<&'static str>,
        captured: RefCell<bool>,
    }

    impl FakeProbe {
        fn ready() -> Self {
            Self {
                missing: Vec::new(),
                captured: RefCell::new(false),
            }
        }
    }

    impl RemoteProbe for FakeProbe {
        fn check(&self, _parsed: &AppShotArgs, capability: &str) -> Result<bool, String> {
            Ok(!self.missing.contains(&capability))
        }

        fn capture(&self, parsed: &AppShotArgs) -> Result<(), String> {
            *self.captured.borrow_mut() = true;
            fs::write(&parsed.output, b"remote capture\n").map_err(|e| e.to_string())
        }
    }

    fn manifest_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("xtask-app-shot-{name}-{}", std::process::id()))
    }

    #[test]
    fn parse_requires_absolute_manifest_and_output() {
        let args = vec![
            "--remote".to_string(),
            "macmini".to_string(),
            "--manifest".to_string(),
            "relative.json".to_string(),
            "--output".to_string(),
            "/tmp/out.png".to_string(),
        ];
        assert!(parse(&args).is_err());
    }

    #[test]
    fn preflight_reports_exact_missing_capability_from_probe() {
        let parsed = AppShotArgs {
            remote: "macmini".into(),
            manifest: PathBuf::from("/tmp/manifest.json"),
            output: PathBuf::from("/tmp/out.png"),
            measure: false,
        };
        let probe = FakeProbe {
            missing: vec!["private-tmux"],
            captured: RefCell::new(false),
        };
        assert_eq!(preflight_missing(&parsed, &probe), vec!["private-tmux"]);
    }

    #[test]
    fn ready_probe_captures_to_requested_output() {
        let dir = manifest_dir("ready");
        fs::create_dir_all(&dir).expect("dir");
        let manifest = dir.join("manifest.json");
        let output = dir.join("capture.txt");
        fs::write(&manifest, b"{}").expect("manifest");
        let parsed = AppShotArgs {
            remote: "macmini".into(),
            manifest,
            output: output.clone(),
            measure: true,
        };
        let probe = FakeProbe::ready();
        assert_eq!(run_with_probe(&parsed, &probe), 0);
        assert_eq!(fs::read(&output).expect("output"), b"remote capture\n");
        assert!(*probe.captured.borrow());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_manifest_fails_before_capture() {
        let dir = manifest_dir("invalid");
        fs::create_dir_all(&dir).expect("dir");
        let manifest = dir.join("manifest.json");
        let output = dir.join("capture.txt");
        fs::write(&manifest, b"[]").expect("manifest");
        let parsed = AppShotArgs {
            remote: "macmini".into(),
            manifest,
            output,
            measure: false,
        };
        let probe = FakeProbe::ready();
        assert_eq!(run_with_probe(&parsed, &probe), 2);
        assert!(!*probe.captured.borrow());
        let _ = fs::remove_dir_all(dir);
    }
}
