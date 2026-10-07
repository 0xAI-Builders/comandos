//! Frozen expectations; replay keeps only actual native actors.
//! Gemelo (D5 del plan 2f, R1 y B3 del pre-flight): dos HOME sembrados igual,
//! cada uno con su tmux privado; el frente muta A y el `cc-dash` Python muta B.
//! Se comparan bytes y archivos tras normalizar lo que depende del reloj.
//!
//! Confinamiento (lo prueban los canarios de `dash_twin.rs`):
//! - Los dos HOME están bajo un temporal corto y tienen `fakebin` confinado
//!   (`oracle::confined_fakebin`): `PATH` = solo él, con falsos que anotan
//!   para agentes, terminales, navegadores, `ssh`, `ssh-copy-id`, `pkill`,
//!   `git`… y una lista corta de herramientas inocuas. Lo demás no existe.
//! - Todo tmux pasa por el guardián del `fakebin` (`-S` al socket privado,
//!   `env -i` con el entorno confinado): el frente lo usa como
//!   `opts.tmux.program` (con `-f /dev/null -S <socket>` en el prefijo), el
//!   Python lo encuentra en su `PATH` y el `systemd-run` falso solo ejecuta
//!   colas de tmux. Así el servidor y todo panel nacen con HOME temporal,
//!   `SHELL=/bin/bash` y sin `DISPLAY`/DBus, lo llame quien lo llame.
//! - El oráculo es `oracle_with`: entorno limpio, red cerrada salvo los puertos
//!   de la prueba, sin bucles de fondo. El frente usa los dobles de
//!   `TestHome::options` (OAuth y avisos falsos, terminal web en el puerto 1,
//!   censo apagado) y `search_path` = `fakebin` de A.
//! - Las llamadas directas de la prueba (`tmux_a`/`tmux_b`) van por
//!   `TestHome::tmux_command` (tmux real con `-S` y el mismo entorno
//!   confinado): no pasan por el guardián, así que no ensucian `tmux_log_*`.
#![allow(dead_code)]
use super::{
    FakeLegacy, Front, TestHome, Wire, front,
    oracle::{FakeCall, Oracle, OracleOpts, calls_in, confined_fakebin, fake_calls, oracle_with},
    request_body,
};
use comandos_server::dash::native::{NativeOptions, quick::scope_program};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Órdenes de tmux que mutan (R9 del pre-flight): las que se comparan entre
/// los dos lados. Las lecturas no (el frente cachea `/state`, el Python no).
pub const TMUX_MUTATORS: &[&str] = &[
    "new-session",
    "new-window",
    "send-keys",
    "kill-session",
    "kill-window",
    "kill-pane",
    "select-window",
    "select-pane",
    "switch-client",
    "set-option",
    "set-environment",
    "rename-session",
    "rename-window",
    "split-window",
    "resize-pane",
    "load-buffer",
    "paste-buffer",
    "delete-buffer",
    "copy-mode",
];

/// Ajuste de las opciones del frente antes de servir (B3).
pub type FrontHook = Box<dyn FnOnce(&mut NativeOptions) + Send>;
type ResponseNormalizer = Option<(&'static str, fn(&TestHome, &str) -> String)>;

/// Opciones de `Twin::start_with`; `Twin::start` usa los valores por omisión.
#[derive(Default)]
pub struct TwinOpts {
    /// Explicit ordinary document effects relative to HOME; never actors or logs.
    pub oracle_files: Vec<&'static str>,
    /// Existing caller normalization applied before freezing/checking output.
    pub response_normalizer: ResponseNormalizer,
    /// Ejecutables extra en el `fakebin` de LOS DOS lados (nombre → texto).
    pub fakebin_extra: Vec<(String, String)>,
    /// Ajuste de `NativeOptions` del frente (tras el confinamiento; se vuelve
    /// a comprobar `assert_private_tmux`).
    pub front: Option<FrontHook>,
    /// Python del oráculo tras cargar `cc-dash` (módulo `dash`), antes de `main()`.
    pub python_prelude: String,
    /// Puertos locales de la prueba a los que el oráculo puede conectar.
    pub allow_ports: Vec<u16>,
    /// Bucles de fondo del oráculo que siguen vivos.
    pub keep_loops: Vec<String>,
    /// Variables extra del oráculo.
    pub oracle_env: Vec<(String, String)>,
}

/// `USER_BIN_DIRS` sin lo que no es del HOME (`/usr/local/bin`).
pub fn home_bin_dirs() -> Vec<String> {
    comandos_runtime::providers::USER_BIN_DIRS
        .iter()
        .filter(|d| d.starts_with('~'))
        .map(|d| (*d).to_owned())
        .collect()
}

pub struct Twin {
    // Orden de los campos = orden de destrucción: primero los servidores
    // (frente, oráculo y su grupo de procesos), después los HOME, cuyo `Drop`
    // mata cada tmux privado por su `-S` y solo entonces borra el directorio.
    pub front: Front,
    oracle: Option<Oracle>,
    tag: String,
    response_normalizer: ResponseNormalizer,
    fixture: Value,
    files: Vec<&'static str>,
    request_id: AtomicUsize,
    observation_id: AtomicUsize,
    _legacy: FakeLegacy,
    /// Las opciones con las que se sirvió el frente (para inspección).
    pub front_options: NativeOptions,
    pub a: TestHome,
    pub b: TestHome,
}

pub struct TwinRun {
    pub front: Wire,
    pub oracle: Wire,
}

impl TwinRun {
    pub fn assert_same(&self) {
        assert_eq!(self.front.status, self.oracle.status, "status");
        assert_eq!(
            normalize(&self.front.text()),
            normalize(&self.oracle.text()),
            "cuerpo"
        );
    }
}

/// Lo que depende del reloj: `term-r<n>`, marcas `ts`/`closedAt`/`updated`/`at`/
/// `heartbeatAt` y nombres `<19 dígitos>-<hex>.json`. Nada más.
pub fn normalize(text: &str) -> String {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let rules = RULES.get_or_init(|| {
        [
            (r"term-r[0-9]+", "term-rN"),
            (
                r#""(ts|closedAt|updated|at|heartbeatAt)": [0-9.eE+-]+"#,
                r#""$1": T"#,
            ),
            (r"[0-9]{19}-[0-9a-f]+\.json", "NS-ID.json"),
        ]
        .into_iter()
        .filter_map(|(re, to)| Regex::new(re).ok().map(|re| (re, to)))
        .collect()
    });
    rules.iter().fold(text.to_string(), |acc, (re, to)| {
        re.replace_all(&acc, *to).into_owned()
    })
}

impl Twin {
    pub async fn start(tag: &str, seed: impl Fn(&TestHome)) -> Option<Twin> {
        Self::start_with(tag, seed, TwinOpts::default()).await
    }

    pub async fn start_with(tag: &str, seed: impl Fn(&TestHome), opts: TwinOpts) -> Option<Twin> {
        if !super::tmux_available() {
            eprintln!("tmux no está instalado: se salta");
            return None;
        }
        let a = TestHome::new_short(&format!("{tag}-a"));
        let b = TestHome::new_short(&format!("{tag}-b"));
        // El confinamiento va ANTES de la siembra: si la siembra arranca un tmux,
        // su servidor ya nace con el entorno confinado.
        let fakebin_a = confined_fakebin(&a, &opts.fakebin_extra);
        confined_fakebin(&b, &opts.fakebin_extra);
        seed(&a);
        seed(&b);
        let response_normalizer = opts.response_normalizer;
        let mut fixture = json!({"fakebin":opts.fakebin_extra,"prelude":opts.python_prelude,
            "keep_loops":opts.keep_loops,"environment":opts.oracle_env});
        if let Some((name, _)) = response_normalizer {
            fixture["response_normalizer"] = json!(name);
        }
        let mut fixture_text = serde_json::to_string(&fixture).unwrap();
        for (i, port) in opts.allow_ports.iter().enumerate() {
            fixture_text = fixture_text.replace(&port.to_string(), &format!("<FIXTURE_PORT_{i}>"));
        }
        let fixture: Value = serde_json::from_str(&fixture_text).unwrap();
        let files = opts.oracle_files;
        let oracle = if matches!(
            std::env::var("COMANDOS_ORACLE").as_deref(),
            Ok("record" | "check")
        ) {
            let reference = super::frozen::reference(&b.root).unwrap();
            let mut extra_env = opts.oracle_env;
            extra_env.push((
                "COMANDOS_ORACLE_REFERENCE_ROOT".into(),
                reference.display().to_string(),
            ));
            Some(
                oracle_with(
                    &b,
                    OracleOpts {
                        fakebin_extra: opts.fakebin_extra,
                        python_prelude: format!(
                            "dash.time.time = lambda: {}\n{}",
                            super::NOW_MS / 1000,
                            opts.python_prelude
                        ),
                        allow_ports: opts.allow_ports,
                        keep_loops: opts.keep_loops,
                        extra_env,
                    },
                )
                .await
                .expect("record/check requires immutable original HTTP"),
            )
        } else {
            None
        };
        let legacy = FakeLegacy::start().await;
        let mut front_options = a.options();
        // tmux = guardián de A con el prefijo privado (`assert_private_tmux`).
        front_options.tmux.program.path = fakebin_a.join("tmux");
        front_options.scope = Some(scope_program(fakebin_a.join("systemd-run")));
        let mut search = fakebin_a.clone().into_os_string();
        search.push(":");
        search.push(a.root.join("bin"));
        front_options.search_path = Some(search);
        // Los respaldos de `providers::which` solo dentro del HOME de A: nunca
        // el `/usr/local/bin` real (el oráculo hace lo mismo con su
        // `sitecustomize.py`, `oracle::SITECUSTOMIZE`).
        front_options.user_bin_dirs = home_bin_dirs();
        // C1 de la revisión de la Tarea 2: lo que el frente lance fuera de tmux
        // (`NativeOptions::program`) solo ve el entorno confinado de A, sin
        // `DISPLAY` (`gui_env_for` lo trata como ausente, como el oráculo).
        let child_env: Vec<(OsString, OsString)> = a
            .confined_env()
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        front_options.child_env = Some(child_env.clone());
        front_options.display = Some(None);
        front_options.ssh = front_options.program(fakebin_a.join("ssh"));
        if let Some(scope) = &mut front_options.scope {
            scope.env_clear = true;
            scope.env = child_env;
        }
        if let Some(hook) = opts.front {
            hook(&mut front_options);
        }
        super::assert_private_tmux(&front_options);
        let front = front(&a, legacy.port, front_options.clone()).await;
        Some(Twin {
            front,
            oracle,
            tag: tag.into(),
            response_normalizer,
            fixture,
            files,
            request_id: AtomicUsize::new(0),
            observation_id: AtomicUsize::new(0),
            _legacy: legacy,
            front_options,
            a,
            b,
        })
    }

    /// La misma petición a los dos lados (cuerpo JSON, token local).
    pub async fn request(&self, method: &str, path: &str, body: &str) -> TwinRun {
        TwinRun {
            front: request_body(self.front.port, method, path, "", body).await,
            oracle: self.original_request(method, path, body).await,
        }
    }

    pub async fn post(&self, path: &str, body: &str) -> TwinRun {
        self.request("POST", path, body).await
    }

    pub async fn get(&self, path: &str) -> TwinRun {
        self.request("GET", path, "").await
    }

    /// Solo al frente (p. ej. para comprobar un efecto que el Python no tiene).
    pub async fn post_front(&self, path: &str, body: &str) -> Wire {
        request_body(self.front.port, "POST", path, "", body).await
    }

    /// Solo al oráculo.
    pub async fn post_oracle(&self, path: &str, body: &str) -> Wire {
        self.original_request("POST", path, body).await
    }

    pub fn source_port(&self) -> Option<u16> {
        self.oracle.as_ref().map(|source| source.port)
    }

    /// Expected source observations, never materialized as logs or processes.
    pub fn observe(&self, operation: &str, args: &Value, run: impl FnOnce() -> Value) -> Value {
        let roots = [("<HOME>", self.b.root.as_path())];
        let input = json!({"source_commit":super::frozen::SOURCE_COMMIT,
            "source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24",
            "python":"CPython 3.10.12", "fixture":self.fixture,
            "after_request":self.request_id.load(Ordering::SeqCst),
            "observation":self.observation_id.fetch_add(1, Ordering::SeqCst),
            "operation":operation,"args":args});
        let input: Value = serde_json::from_slice(&comandos_oracle::normalize(
            &serde_json::to_vec(&input).unwrap(),
            &roots,
        ))
        .unwrap();
        let output = comandos_oracle::oracle_at(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            &format!("server-twin-{}", self.tag),
            &input,
            || {
                assert!(
                    self.oracle.is_some(),
                    "original observation requires record/check"
                );
                Ok(comandos_oracle::normalize(
                    &serde_json::to_vec(&run()).unwrap(),
                    &roots,
                ))
            },
        );
        serde_json::from_slice(&comandos_oracle::restore(&output, &roots)).unwrap()
    }

    pub fn original_file(&self, name: &str) -> Option<Vec<u8>> {
        serde_json::from_value(self.observe("file", &json!(name), || {
            json!(std::fs::read(self.b.root.join(name)).ok())
        }))
        .unwrap()
    }

    async fn original_request(&self, method: &str, path: &str, body: &str) -> Wire {
        let capsule = self.b.root.join(".oracle/twin-effects");
        super::http_golden::copy_domain(&self.b.root, &capsule, &self.files);
        let roots = [("<HOME>", self.b.root.as_path())];
        let input = json!({"source_commit":super::frozen::SOURCE_COMMIT,
            "source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24",
            "python":"CPython 3.10.12", "fixture":self.fixture,
            "sequence":self.request_id.fetch_add(1,Ordering::SeqCst),
            "request":{"method":method,"path":path,"body":body}, "effect_files":self.files});
        let input: Value = serde_json::from_slice(&comandos_oracle::normalize(
            &serde_json::to_vec(&input).unwrap(),
            &roots,
        ))
        .unwrap();
        // Yield to actual private health actors on current-thread runtimes.
        let actual = if let Some(source) = &self.oracle {
            Some(request_body(source.port, method, path, "", body).await)
        } else {
            None
        };
        let output=comandos_oracle::text_with_tree_at(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            &format!("server-twin-{}",self.tag),&input,&capsule,&roots,
            || {
                let mut wire=actual.ok_or("original HTTP requires record/check")?;
                if let Some((_,normalize)) = self.response_normalizer {
                    wire.body = normalize(&self.b, &wire.text()).into_bytes();
                }
                super::http_golden::copy_domain(&self.b.root,&capsule,&self.files);
                serde_json::to_string(&json!({"status":wire.status,"headers":wire.headers,"body":comandos_oracle::normalize(&wire.body,&roots)})).map_err(|error|error.to_string())
            },
        ).unwrap();
        if self.oracle.is_none() {
            super::http_golden::copy_domain(&capsule, &self.b.root, &self.files);
        }
        let output: Value = serde_json::from_str(&output).unwrap();
        Wire {
            status: output["status"].as_u64().unwrap().try_into().unwrap(),
            headers: serde_json::from_value(output["headers"].clone()).unwrap(),
            body: comandos_oracle::restore(
                &serde_json::from_value::<Vec<u8>>(output["body"].clone()).unwrap(),
                &roots,
            ),
        }
    }

    /// Cada archivo (relativo a `~/.claude/hooks`, o `~/…`) igual en A y B tras
    /// `normalize`; ausente en los dos cuenta como igual.
    pub fn files_equal(&self, rel: &[&str]) -> Result<(), String> {
        for name in rel {
            let path = |home: &TestHome| match name.strip_prefix("~/") {
                Some(rest) => home.root.join(rest),
                None => home.hooks().join(name),
            };
            let read = |home: &TestHome| {
                std::fs::read_to_string(path(home))
                    .ok()
                    .map(|t| normalize(&t))
            };
            let (front, oracle) = (read(&self.a), read(&self.b));
            if front != oracle {
                return Err(format!("{name}: frente {front:?} ≠ oráculo {oracle:?}"));
            }
        }
        Ok(())
    }

    pub fn tmux_a(&self, args: &[&str]) -> String {
        super::run_tmux(&self.a, args)
    }

    pub fn tmux_b(&self, args: &[&str]) -> String {
        self.observe("tmux-read", &json!(args), || {
            json!(super::run_tmux(&self.b, args))
        })
        .as_str()
        .unwrap()
        .to_owned()
    }

    /// Llamadas a tmux del frente (A) por el guardián, sin el prefijo
    /// `-f /dev/null -S <socket>` del frente: comparables con las del Python.
    pub fn tmux_log_a(&self) -> Vec<Vec<String>> {
        tmux_log(&self.a)
    }

    pub fn tmux_log_b(&self) -> Vec<Vec<String>> {
        serde_json::from_value(self.observe("tmux-log", &Value::Null, || json!(tmux_log(&self.b))))
            .unwrap()
    }

    /// Solo las órdenes mutadoras (`TMUX_MUTATORS`), con `normalize` aplicado.
    pub fn tmux_mutations_a(&self) -> Vec<Vec<String>> {
        mutations(tmux_log(&self.a))
    }

    pub fn tmux_mutations_b(&self) -> Vec<Vec<String>> {
        mutations(self.tmux_log_b())
    }

    /// Llamadas a los falsos del `fakebin` de cada lado.
    pub fn fake_calls_a(&self) -> Vec<FakeCall> {
        fake_calls(&self.a.root)
    }

    pub fn fake_calls_b(&self) -> Vec<FakeCall> {
        self.observe("fake-calls", &Value::Null, || {
            json!(
                fake_calls(&self.b.root)
                    .iter()
                    .map(|call| json!([call.name, call.args]))
                    .collect::<Vec<_>>()
            )
        })
        .as_array()
        .unwrap()
        .iter()
        .map(|call| FakeCall {
            name: call[0].as_str().unwrap().into(),
            args: serde_json::from_value(call[1].clone()).unwrap(),
        })
        .collect()
    }
}

/// `<root>/tmux.log` del guardián, sin las opciones globales `-f`/`-S`/`-L`
/// que el frente antepone (el Python no las pasa).
pub fn tmux_log(home: &TestHome) -> Vec<Vec<String>> {
    calls_in(&home.root.join("tmux.log"))
        .into_iter()
        .map(|call| {
            let mut args = call.args.as_slice();
            while let [flag, _value, rest @ ..] = args
                && matches!(flag.as_str(), "-f" | "-S" | "-L")
            {
                args = rest;
            }
            args.to_vec()
        })
        .collect()
}

/// El stdin de cada `load-buffer` que pasó por el guardián de `home`
/// (`<root>/tmux-stdin.log`), en orden y byte a byte.
pub fn tmux_stdin(home: &TestHome) -> Vec<Vec<u8>> {
    let bytes = std::fs::read(home.root.join("tmux-stdin.log")).unwrap_or_default();
    let mut out = Vec::new();
    let mut rest = bytes.as_slice();
    while let Some(at) = rest.windows(3).position(|w| w == b"\0\x1e\0") {
        out.push(rest[..at].to_vec());
        rest = &rest[at + 3..];
    }
    out
}

fn mutations(log: Vec<Vec<String>>) -> Vec<Vec<String>> {
    log.into_iter()
        .filter(|args| {
            args.first()
                .is_some_and(|verb| TMUX_MUTATORS.contains(&verb.as_str()))
        })
        .map(|args| args.iter().map(|a| normalize(a)).collect())
        .collect()
}
