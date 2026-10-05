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
use std::sync::OnceLock;

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

/// Opciones de `Twin::start_with`; `Twin::start` usa los valores por omisión.
#[derive(Default)]
pub struct TwinOpts {
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

pub struct Twin {
    // Orden de los campos = orden de destrucción: primero los servidores
    // (frente, oráculo y su grupo de procesos), después los HOME, cuyo `Drop`
    // mata cada tmux privado por su `-S` y solo entonces borra el directorio.
    pub front: Front,
    pub oracle: Oracle,
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
        let oracle = oracle_with(
            &b,
            OracleOpts {
                fakebin_extra: opts.fakebin_extra,
                python_prelude: opts.python_prelude,
                allow_ports: opts.allow_ports,
                keep_loops: opts.keep_loops,
                extra_env: opts.oracle_env,
            },
        )
        .await?;
        let legacy = FakeLegacy::start().await;
        let mut front_options = a.options();
        // tmux = guardián de A con el prefijo privado (`assert_private_tmux`).
        front_options.tmux.program.path = fakebin_a.join("tmux");
        front_options.scope = Some(scope_program(fakebin_a.join("systemd-run")));
        let mut search = fakebin_a.clone().into_os_string();
        search.push(":");
        search.push(a.root.join("bin"));
        front_options.search_path = Some(search);
        if let Some(hook) = opts.front {
            hook(&mut front_options);
        }
        super::assert_private_tmux(&front_options);
        let front = front(&a, legacy.port, front_options.clone()).await;
        Some(Twin {
            front,
            oracle,
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
            oracle: request_body(self.oracle.port, method, path, "", body).await,
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
        request_body(self.oracle.port, "POST", path, "", body).await
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
        super::run_tmux(&self.b, args)
    }

    /// Llamadas a tmux del frente (A) por el guardián, sin el prefijo
    /// `-f /dev/null -S <socket>` del frente: comparables con las del Python.
    pub fn tmux_log_a(&self) -> Vec<Vec<String>> {
        tmux_log(&self.a)
    }

    pub fn tmux_log_b(&self) -> Vec<Vec<String>> {
        tmux_log(&self.b)
    }

    /// Solo las órdenes mutadoras (`TMUX_MUTATORS`), con `normalize` aplicado.
    pub fn tmux_mutations_a(&self) -> Vec<Vec<String>> {
        mutations(tmux_log(&self.a))
    }

    pub fn tmux_mutations_b(&self) -> Vec<Vec<String>> {
        mutations(tmux_log(&self.b))
    }

    /// Llamadas a los falsos del `fakebin` de cada lado.
    pub fn fake_calls_a(&self) -> Vec<FakeCall> {
        fake_calls(&self.a.root)
    }

    pub fn fake_calls_b(&self) -> Vec<FakeCall> {
        fake_calls(&self.b.root)
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

fn mutations(log: Vec<Vec<String>>) -> Vec<Vec<String>> {
    log.into_iter()
        .filter(|args| {
            args.first()
                .is_some_and(|verb| TMUX_MUTATORS.contains(&verb.as_str()))
        })
        .map(|args| args.iter().map(|a| normalize(a)).collect())
        .collect()
}
