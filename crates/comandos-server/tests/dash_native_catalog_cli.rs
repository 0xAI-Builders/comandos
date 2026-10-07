//! Catálogo de CLIs, cadenas y modelos de OpenCode nativos (plan 2f-3,
//! Tarea 4) contra el `cc-dash` Python (D8: `shortcuts` y el marcador
//! `COMANDOS_CODEX_ORIGINAL` del lanzador YOLO).
//!
//! Confinamiento: los CLIs del catálogo (`claude`, `codex`, `grok`,
//! `opencode`, `agy`) son guiones del `fakebin` del gemelo que responden
//! `--version`, `--help` y `models` con texto fijo y anotan cada llamada; el
//! `codex` de cada lado es además un lanzador con el marcador hacia un «ELF»
//! falso de su HOME. El `PATH` de los dos lados es solo ese `fakebin` (el
//! frente, `search_path` + `user_bin_dirs` del HOME), y una prueba canario
//! comprueba que ningún nombre resuelve fuera del HOME temporal. Las cadenas
//! viven en `$XDG_CONFIG_HOME` del entorno confinado (el HOME temporal). Sin
//! tmux salvo el privado de cada HOME (la consulta de `?pane=`).
mod support;

use comandos_runtime::providers;
use serde_json::{Value, json};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use support::{
    FakeLegacy, Front, TestHome, front,
    frozen_twin::{Twin, TwinOpts},
    get,
    oracle::OracleOpts,
};

const CHAIN_FILES: &[&str] = &[
    ".config/comandos/cadenas/nueva-cadena.md",
    ".config/comandos/cadenas/nueva-cadena-2.md",
    ".config/comandos/cadenas/deploy.md",
    ".config/comandos/cadenas/mal.md",
    ".config/comandos/cadenas/revision-diaria.md",
    ".config/comandos/cadenas/revision-diaria-2.md",
];
fn original_cli_log(t: &Twin) -> String {
    t.observe("cli-log", &Value::Null, || {
        json!(std::fs::read_to_string(t.b.root.join("cli.log")).unwrap_or_default())
    })
    .as_str()
    .unwrap()
    .to_owned()
}
const CLAUDE_HELP: &str = "Usage: claude [options] [command] [prompt]

Claude Code - starts an interactive session by default

Options:
  --dangerously-skip-permissions                    Bypass all permission checks. Recommended only for sandboxes.
  --model <model>                                   Model for the current session
  -c, --continue                                    Continue the most recent conversation

Commands:
  mcp                                               Configure and manage MCP servers
  update|upgrade                                    Check for updates and install if available
";

const CODEX_HELP: &str = "Codex CLI

Usage: codex [OPTIONS] [PROMPT]

Commands:
  exec        Run Codex non-interactively [aliases: e]
  resume      Resume a previous interactive session

Options:
  -s, --sandbox <SANDBOX_MODE>
          Select the sandbox policy

          [possible values: read-only, workspace-write, danger-full-access]

  -a, --ask-for-approval <APPROVAL_POLICY>
          Configure when the model requires human approval

          Possible values:
          - untrusted:  Only run trusted commands
          - on-request: The model decides when to ask
          - never:      Never ask

      --dangerously-bypass-approvals-and-sandbox
          Skip all confirmation prompts and execute commands without sandboxing
";

/// Guion de un CLI falso: anota su argv en `$HOME/cli.log` y responde.
fn fake_cli(version: &str, help: &str, extra: &str) -> String {
    format!(
        "#!/bin/sh\n\
         printf '%s\\n' \"$(basename \"$0\") $*\" >> \"$HOME/cli.log\"\n\
         case \"$1\" in\n\
         --version) echo {version};;\n\
         --help) cat <<'AYUDA'\n{help}AYUDA\n;;\n\
         {extra}\
         esac\n"
    )
}

const OPENCODE_MODELS: &str = "models) cat <<'MODELOS'\n\
opencode/big-pickle\n\
{\"id\": \"big-pickle\", \"providerID\": \"opencode\", \"name\": \"Big Pickle\", \"status\": \"active\", \"capabilities\": {\"toolcall\": true, \"input\": {\"text\": true}, \"output\": {\"text\": true}}, \"limit\": {\"context\": 200000}}\n\
anthropic/claude-x\n\
{\"id\": \"claude-x\", \"providerID\": \"anthropic\", \"name\": \"Claude X\", \"capabilities\": {\"toolcall\": true}, \"limit\": {\"context\": \"1000\"}}\n\
groq/whisper\n\
{\"id\": \"whisper\", \"providerID\": \"groq\", \"capabilities\": {\"toolcall\": false}}\n\
openrouter/free-one\n\
{\"id\": \"free-one\", \"providerID\": \"openrouter\", \"status\": \"deprecated\"}\n\
broken {\"id\": \n\
{\"id\": \"~alias\", \"providerID\": \"opencode\"}\n\
MODELOS\n\
if [ -f \"$HOME/oc-second\" ]; then echo '{\"id\": \"late\", \"providerID\": \"zz\"}'; fi\n\
touch \"$HOME/oc-second\";;\n";

fn fakes() -> Vec<(String, String)> {
    vec![
        ("claude".into(), fake_cli("2.1.286", CLAUDE_HELP, "")),
        ("grok".into(), fake_cli("'grok 1.0.44'", "", "")),
        (
            "agy".into(),
            fake_cli(
                "v1.2.14",
                "Usage of agy:\n  -yolo\n    \tAuto-approve all tool executions\n",
                "",
            ),
        ),
        ("opencode".into(), fake_cli("1.18.33", "", OPENCODE_MODELS)),
    ]
}

/// «ELF» falso de `codex` con algunos comandos del catálogo, común a los dos
/// lados (el marcador del lanzador lleva una ruta absoluta, y el `fakebin` del
/// oráculo se reinstala al arrancarlo: no puede apuntar a cada HOME).
struct SharedElf(std::path::PathBuf);

impl SharedElf {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lane2f-elf-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("codex"),
            b"\x7fELF ... /status /model fork ... review\0",
        )
        .unwrap();
        Self(dir)
    }

    /// `codex`: lanzador YOLO con el marcador hacia el «ELF».
    fn launcher(&self) -> String {
        format!(
            "#!/bin/sh\n# COMANDOS_CODEX_ORIGINAL={}\n{}",
            json!(self.0.join("codex").display().to_string()),
            fake_cli("'codex-cli 0.159.2'", CODEX_HELP, "").trim_start_matches("#!/bin/sh\n")
        )
    }

    fn opts(&self) -> TwinOpts {
        let mut fakebin = fakes();
        fakebin.push(("codex".into(), self.launcher()));
        TwinOpts {
            fakebin_extra: fakebin,
            fixture_aliases: vec![("<SHARED_ELF>".into(), self.0.clone())],
            oracle_files: CHAIN_FILES.to_vec(),
            ..TwinOpts::default()
        }
    }
}

impl Drop for SharedElf {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn seed_snapshot(home: &TestHome) {
    home.write(
        "model-watch.json",
        &json!({
            "checkedAt": 1759680000.5,
            "versions": {"claude": "2.1.286", "codex": "0.150.0", "grok": "1.0.44", "agy": "1.2.14"},
            "discovered": {"claude": ["claude-opus-5-6", "claude-opus-5-5", "claude-haiku-4-5-20251001"],
                           "codex": ["gpt-5.6", "gpt-5.10"], "grok": ["grok-4.7"]},
            "newSince": {"claude": {"models": ["claude-opus-5-6"], "at": 1, "cli": "2.1.286"},
                         "grok": "no-dict"},
        })
        .to_string(),
    );
}

fn seed_accounts(home: &TestHome) {
    std::fs::create_dir_all(home.root.join(".claude-accounts/trabajo")).unwrap();
    std::fs::create_dir_all(home.root.join(".claude-accounts/x.lock")).unwrap();
}

fn unhome(home: &TestHome, text: &str) -> String {
    text.replace(&home.root.display().to_string(), "<HOME>")
}

async fn same(t: &Twin, path: &str) -> String {
    let run = t.get(path).await;
    assert_eq!(run.front.status, run.oracle.status, "{path}");
    let front = unhome(&t.a, &run.front.text());
    assert_eq!(front, unhome(&t.b, &run.oracle.text()), "{path}");
    front
}

fn opts() -> TwinOpts {
    TwinOpts {
        fakebin_extra: fakes(),
        oracle_files: CHAIN_FILES.to_vec(),
        ..TwinOpts::default()
    }
}

/// Canario: con las opciones del gemelo ningún binario del catálogo (ni `sh`)
/// resuelve fuera del HOME temporal, ni en el frente ni en el oráculo.
#[tokio::test]
async fn catalog_binaries_resolve_inside_the_home() {
    let elf = SharedElf::new("canary");
    let Some(t) = Twin::start_with("cat-canary", |_| {}, elf.opts()).await else {
        return;
    };
    let names = ["claude", "codex", "grok", "opencode", "agy", "gemini", "sh"];
    for name in names {
        let hit = providers::which_in_dirs(
            name,
            t.front_options.search_path.as_deref(),
            &t.front_options.home,
            &t.front_options.user_bin_dirs,
        );
        if let Some(hit) = hit {
            assert!(hit.starts_with(&t.a.root), "{name} -> {}", hit.display());
        }
    }
    let code = format!(
        "import providers, shutil\nprint([providers.which(n) for n in {names:?}], shutil.which('opencode'))\n"
    );
    let out = support::http_golden::dash_files(
        &t.b,
        "server-catalog-resolution",
        &[],
        &code,
        &OracleOpts::default(),
    );
    assert!(!out.contains("/home/someguy/.local"), "{out}");
    assert!(!out.contains(".opencode/bin"), "{out}");
}

#[tokio::test]
async fn commands_catalog_matches_python() {
    let seed = |home: &TestHome| {
        seed_snapshot(home);
        seed_accounts(home);
    };
    let elf = SharedElf::new("snap");
    let Some(t) = Twin::start_with("cat-snap", seed, elf.opts()).await else {
        return;
    };
    let body = same(&t, "/commands/catalog?session=s&pane=%250").await;
    let doc: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(doc["target"], json!({"session": "s", "pane": "%0"}));
    assert_eq!(doc["cliInPane"], "");
    assert_eq!(doc["versionsAt"], json!(1759680000.5));
    let clis = doc["catalog"]["clis"].as_array().unwrap();
    let codex = clis.iter().find(|c| c["id"] == "codex").unwrap();
    // D8: el lanzador lleva al «ELF» y solo quedan sus comandos; atajos de Codex.
    assert!(codex["detected"]["found"].as_i64().unwrap() > 0);
    assert_eq!(codex["start"]["shortcuts"].as_array().unwrap().len(), 3);
    let claude = clis.iter().find(|c| c["id"] == "claude").unwrap();
    assert_eq!(
        claude["start"]["rows"][1]["text"]
            .as_str()
            .unwrap()
            .split(' ')
            .count(),
        2
    );
    // Segunda vez, desde las cachés (sin volver a leer `--help`).
    same(&t, "/commands/catalog").await;
    same(&t, "/commands/catalog?session=una-sesion-muy-larga-que-pasa-de-ochenta-caracteres-y-se-corta-justo-en-ochenta-xyz&pane=nope").await;
    let helps = |home: &TestHome| {
        std::fs::read_to_string(home.root.join("cli.log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.ends_with("--help"))
            .count()
    };
    let expected_helps = original_cli_log(&t)
        .lines()
        .filter(|line| line.ends_with(" --help"))
        .count();
    assert_eq!(helps(&t.a), expected_helps);
    // Las tres ayudas con texto se guardan; las vacías (grok, opencode) se
    // vuelven a pedir en cada petición, como en el Python.
    assert_eq!(helps(&t.a), 3 + 2 * 3);
}

/// Sin snapshot del vigilante el Python sondea `--version` (una vez, 600 s):
/// todo igual salvo `versionsAt` (la hora del sondeo de cada lado).
#[tokio::test]
async fn commands_catalog_without_snapshot_matches_python() {
    let elf = SharedElf::new("nosnap");
    let Some(t) = Twin::start_with("cat-nosnap", |_| {}, elf.opts()).await else {
        return;
    };
    for _ in 0..2 {
        let run = t.get("/commands/catalog").await;
        assert_eq!(run.front.status, 200);
        assert_eq!(run.oracle.status, 200);
        let mut a: Value = serde_json::from_str(&unhome(&t.a, &run.front.text())).unwrap();
        let mut b: Value = serde_json::from_str(&unhome(&t.b, &run.oracle.text())).unwrap();
        assert!(a["versionsAt"].as_f64().unwrap() > 1.0e9);
        a["versionsAt"] = Value::Null;
        b["versionsAt"] = Value::Null;
        assert_eq!(a, b);
        assert_eq!(a["catalog"]["clis"][0]["version"]["installed"], "2.1.286");
        assert_eq!(a["catalog"]["clis"][1]["version"]["installed"], "0.159.2");
    }
    let versions = |home: &TestHome| {
        std::fs::read_to_string(home.root.join("cli.log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.ends_with("--version"))
            .count()
    };
    assert_eq!(versions(&t.a), 5);
    assert_eq!(
        original_cli_log(&t)
            .lines()
            .filter(|line| line.ends_with(" --version"))
            .count(),
        5
    );
}

/// `?refresh=1` fuerza un ciclo del vigilante (Tarea 6): ya no declina (el
/// ciclo forzado se prueba en `dash_native_background.rs`); sin `refresh` la
/// ruta no corre ningún ciclo.
#[tokio::test]
async fn catalog_without_refresh_runs_no_cycle() {
    let home = TestHome::new("cat-refresh");
    let mut opts = home.options();
    opts.user_bin_dirs = support::twin::home_bin_dirs();
    let legacy = FakeLegacy::start().await;
    let server = front(&home, legacy.port, opts).await;
    let wire = get(server.port, "/commands/catalog").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(legacy.requests().is_empty());
    assert!(!home.hooks().join("model-watch.json").exists());
    let wire = get(server.port, "/commands/catalog?refresh=1&session=s").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(legacy.requests().is_empty());
    assert!(home.hooks().join("model-watch.json").exists());
    server.stop().await;
}

const CHAIN_DIR: &str = ".config/comandos/cadenas";

fn seed_chains(home: &TestHome) {
    let dir = home.root.join(CHAIN_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("deploy.md"),
        "# Deploy\n\n1. shell: git pull\n2. pane: make\n",
    )
    .unwrap();
    std::fs::write(dir.join("Mala.md"), "# x\n").unwrap();
    std::fs::write(dir.join("rota.md"), "# R\nesto no es un paso\n").unwrap();
}

#[tokio::test]
async fn chains_routes_match_python() {
    let Some(t) = Twin::start_with("chains", seed_chains, opts()).await else {
        return;
    };
    same(&t, "/chains").await;
    let posts = [
        r#"{"name": "Nueva cadena", "steps": [{"kind": "shell", "text": " ls "}]}"#,
        r#"{"name": "Nueva cadena", "steps": [{"kind": "pane", "text": "pwd"}]}"#,
        r#"{"name": "Fija", "slug": "deploy", "steps": [{"kind": "pane", "text": "make test"}]}"#,
        r#"{"name": "Mal", "slug": "Con Espacios", "steps": [{"kind": "pane", "text": "x"}]}"#,
        r#"{"name": "", "steps": [{"kind": "pane", "text": "x"}]}"#,
        r#"{"name": "Sin pasos", "steps": []}"#,
        r#"{"name": "Tipo", "steps": [{"kind": "bash", "text": "x"}]}"#,
        r#"{"name": "Control", "steps": [{"kind": "shell", "text": "a\u0007b"}]}"#,
    ];
    for body in posts {
        let run = t.post("/chains", body).await;
        assert_eq!(run.front.status, run.oracle.status, "{body}");
        assert_eq!(run.front.text(), run.oracle.text(), "{body}");
    }
    t.files_equal(&[
        "~/.config/comandos/cadenas/nueva-cadena.md",
        "~/.config/comandos/cadenas/nueva-cadena-2.md",
        "~/.config/comandos/cadenas/deploy.md",
        "~/.config/comandos/cadenas/mal.md",
    ])
    .unwrap();
    same(&t, "/chains").await;
}

/// M5: nombres con acentos se sirven en el frente (NFKD en `slugify`): el
/// archivo, la respuesta (`\u00f3` por `ensure_ascii`) y el listado son los
/// del Python.
#[tokio::test]
async fn chains_with_accents_match_python() {
    let Some(t) = Twin::start_with("chains-acc", |_| {}, opts()).await else {
        return;
    };
    let body =
        r#"{"name": "Revisión diaria", "steps": [{"kind": "pane", "text": "git status · ñ"}]}"#;
    for _ in 0..2 {
        let run = t.post("/chains", body).await;
        assert_eq!(run.front.status, 200, "{}", run.front.text());
        assert_eq!(run.front.text(), run.oracle.text());
    }
    t.files_equal(&[
        "~/.config/comandos/cadenas/revision-diaria.md",
        "~/.config/comandos/cadenas/revision-diaria-2.md",
    ])
    .unwrap();
    let listed = same(&t, "/chains").await;
    assert!(listed.contains("Revisi\\u00f3n diaria"), "{listed}");
}

/// Un archivo en lugar del directorio de cadenas: `FileExistsError` (500).
#[tokio::test]
async fn chains_post_os_error_matches_python() {
    let seed = |home: &TestHome| {
        std::fs::create_dir_all(home.root.join(".config/comandos")).unwrap();
        std::fs::write(home.root.join(CHAIN_DIR), "no soy un directorio").unwrap();
    };
    let Some(t) = Twin::start_with(
        "chains-os",
        seed,
        TwinOpts {
            oracle_files: Vec::new(),
            ..opts()
        },
    )
    .await
    else {
        return;
    };
    let run = t
        .post(
            "/chains",
            r#"{"name": "X", "steps": [{"kind": "shell", "text": "y"}]}"#,
        )
        .await;
    assert_eq!(run.front.status, 500);
    assert_eq!(run.front.text(), run.oracle.text());
    same(&t, "/chains").await;
}

/// `~/.claude/hooks/providers.env`: sin él, el `. archivo` del `sh -c` es un
/// error fatal de `sh` y la lista sale vacía (igual en los dos lados).
fn seed_providers_env(home: &TestHome) {
    home.write("providers.env", "# claves de prueba\n");
}

#[tokio::test]
async fn opencode_models_cold_and_cached_match_python() {
    let Some(t) = Twin::start_with("oc-models", seed_providers_env, opts()).await else {
        return;
    };
    let first = same(&t, "/opencode/models").await;
    let doc: Value = serde_json::from_str(&first).unwrap();
    let providers = doc["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 2, "{first}");
    // Desde la caché (900 s): el segundo `models` no corre.
    assert_eq!(same(&t, "/opencode/models?x").await, first);
    let models = |home: &TestHome| {
        std::fs::read_to_string(home.root.join("cli.log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("opencode models"))
            .count()
    };
    assert_eq!(models(&t.a), 1);
    assert_eq!(
        original_cli_log(&t)
            .lines()
            .filter(|line| line.starts_with("opencode models"))
            .count(),
        1
    );
}

/// Un `opencode` que no imprime nada: `[]` (la lista vacía no se guarda).
#[tokio::test]
async fn opencode_models_empty_output_matches_python() {
    let Some(t) = Twin::start("oc-none", seed_providers_env).await else {
        return;
    };
    // El `opencode` que anota del `fakebin` no imprime nada: `[]`.
    let body = same(&t, "/opencode/models").await;
    assert_eq!(body, r#"{"providers": []}"#);
}

// ---------------------------------------------------------------------------
// Frente solo: lo incierto se recuerda (revisión de la Tarea 4)
// ---------------------------------------------------------------------------

/// Reloj del frente en segundos (bits de `f64`), movible desde la prueba.
struct Clock(Arc<AtomicU64>);

impl Clock {
    fn at(seconds: f64) -> Self {
        Self(Arc::new(AtomicU64::new(seconds.to_bits())))
    }

    fn set(&self, seconds: f64) {
        self.0.store(seconds.to_bits(), Ordering::SeqCst);
    }
}

/// Un frente solo (sin oráculo): su `PATH` es `<HOME>/bin` con los guiones
/// dados y `sh`; los CLIs anotan en `<HOME>/cli.log`. Lo que declina va al
/// heredado falso (`{"legacy": true}`).
async fn lone_front(
    home: &TestHome,
    scripts: &[(&str, String)],
    clock: &Clock,
    repo: Option<std::path::PathBuf>,
) -> (FakeLegacy, Front) {
    let bin = home.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for (name, text) in scripts {
        let path = bin.join(name);
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::os::unix::fs::symlink("/bin/sh", bin.join("sh")).unwrap();
    let mut opts = home.options();
    opts.user_bin_dirs = support::twin::home_bin_dirs();
    if repo.is_some() {
        opts.repo_root = repo;
    }
    let clock = Arc::clone(&clock.0);
    opts.clock_seconds = Arc::new(move || f64::from_bits(clock.load(Ordering::SeqCst)));
    let legacy = FakeLegacy::start().await;
    let server = front(home, legacy.port, opts).await;
    (legacy, server)
}

/// Líneas de `<HOME>/cli.log` que empiezan por `prefix` (tras dejar terminar
/// cualquier tarea lanzada en segundo plano).
async fn calls(home: &TestHome, prefix: &str) -> usize {
    tokio::time::sleep(Duration::from_millis(300)).await;
    std::fs::read_to_string(home.root.join("cli.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with(prefix))
        .count()
}

const LEGACY: &str = r#"{"legacy": true}"#;
const T0: f64 = 1_759_680_000.0;

/// I1: una salida de `opencode models` que el port no reproduce (`NaN`)
/// declina, y las peticiones siguientes declinan sin relanzar el refresco
/// (que reescribe la caché de proveedores de opencode) hasta que vence la
/// frescura de 900 s; entonces se sondea una vez más.
#[tokio::test]
async fn opencode_models_unsure_declines_without_relaunching() {
    let home = TestHome::new("oc-unsure");
    seed_providers_env(&home);
    let clock = Clock::at(T0);
    let nan = "models) echo '{\"id\": \"x\", \"providerID\": \"p\", \"n\": NaN}';;\n";
    let (legacy, server) = lone_front(
        &home,
        &[("opencode", fake_cli("1.0.0", "", nan))],
        &clock,
        None,
    )
    .await;
    for _ in 0..4 {
        assert_eq!(get(server.port, "/opencode/models").await.text(), LEGACY);
    }
    assert_eq!(calls(&home, "opencode models").await, 1);
    clock.set(T0 + 901.0);
    for _ in 0..3 {
        assert_eq!(get(server.port, "/opencode/models").await.text(), LEGACY);
    }
    assert_eq!(calls(&home, "opencode models").await, 2);
    assert_eq!(legacy.requests().len(), 7);
    server.stop().await;
}

/// I2: una ayuda que el port no interpreta con certeza (U+001C, que `\s` de
/// Python cuenta como espacio) declina, y se recuerda por la llave del
/// ejecutable: la segunda petición declina sin volver a correr `--help`. Si
/// el ejecutable cambia, se vuelve a pedir.
#[tokio::test]
async fn catalog_unsure_help_is_remembered() {
    let home = TestHome::new("cat-help-unsure");
    seed_snapshot(&home);
    let clock = Clock::at(T0);
    let help = "Usage: claude [options]\n\nOptions:\n  -x, --rara  marca\u{1c}rara\n";
    let (legacy, server) = lone_front(
        &home,
        &[("claude", fake_cli("2.1.286", help, ""))],
        &clock,
        None,
    )
    .await;
    for _ in 0..3 {
        assert_eq!(get(server.port, "/commands/catalog").await.text(), LEGACY);
    }
    assert_eq!(calls(&home, "claude --help").await, 1);
    // Otro ejecutable (otra llave): se vuelve a sondear una vez.
    let exe = home.root.join("bin/claude");
    let text = std::fs::read_to_string(&exe).unwrap();
    std::fs::write(&exe, format!("{text}# cambio\n")).unwrap();
    for _ in 0..2 {
        assert_eq!(get(server.port, "/commands/catalog").await.text(), LEGACY);
    }
    assert_eq!(calls(&home, "claude --help").await, 2);
    assert_eq!(legacy.requests().len(), 5);
    server.stop().await;
}

/// I2: sin snapshot, una salida de `--version` incierta declina y se recuerda
/// los 600 s del sondeo de respaldo del Python; después se sondea otra vez.
#[tokio::test]
async fn catalog_unsure_version_is_remembered() {
    let home = TestHome::new("cat-version-unsure");
    let clock = Clock::at(T0);
    let (legacy, server) = lone_front(
        &home,
        &[("claude", fake_cli("'2.1.0\x1c'", CLAUDE_HELP, ""))],
        &clock,
        None,
    )
    .await;
    for _ in 0..3 {
        assert_eq!(get(server.port, "/commands/catalog").await.text(), LEGACY);
    }
    assert_eq!(calls(&home, "claude --version").await, 1);
    clock.set(T0 + 601.0);
    for _ in 0..2 {
        assert_eq!(get(server.port, "/commands/catalog").await.text(), LEGACY);
    }
    assert_eq!(calls(&home, "claude --version").await, 2);
    // Nunca se llegó a pedir la ayuda: se declinó antes.
    assert_eq!(calls(&home, "claude --help").await, 0);
    assert_eq!(legacy.requests().len(), 5);
    server.stop().await;
}

/// Copia de `config/` del repo con un modelo de `providers.json` cambiado.
fn repo_with_model(home: &TestHome, model: Value) -> std::path::PathBuf {
    let repo = home.root.join("repo");
    std::fs::create_dir_all(repo.join("config")).unwrap();
    for entry in std::fs::read_dir(support::repo().join("config")).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            std::fs::copy(&path, repo.join("config").join(path.file_name().unwrap())).unwrap();
        }
    }
    let file = repo.join("config/providers.json");
    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    doc["motors"]["claude"]["models"]
        .as_array_mut()
        .unwrap()
        .push(model);
    std::fs::write(&file, doc.to_string()).unwrap();
    repo
}

/// M1: un id de modelo que no es hashable (`[1]`) hace que el
/// `_registry_model_ids` del Python lance `TypeError` (500) después del
/// sondeo de versiones: el frente declina antes de ejecutar nada. Un id
/// escalar que no es texto (`7`) se salta como en `latest_models`.
#[tokio::test]
async fn catalog_unhashable_model_id_declines_before_probing() {
    let home = TestHome::new("cat-unhashable");
    let repo = repo_with_model(&home, json!({"id": [1], "name": "lista"}));
    let clock = Clock::at(T0);
    let (legacy, server) = lone_front(
        &home,
        &[("claude", fake_cli("2.1.286", CLAUDE_HELP, ""))],
        &clock,
        Some(repo),
    )
    .await;
    assert_eq!(get(server.port, "/commands/catalog").await.text(), LEGACY);
    assert_eq!(calls(&home, "claude").await, 0);
    assert_eq!(legacy.requests().len(), 1);
    server.stop().await;

    let home = TestHome::new("cat-scalar-id");
    let repo = repo_with_model(&home, json!({"id": 7, "name": "número"}));
    let (_legacy, server) = lone_front(
        &home,
        &[("claude", fake_cli("2.1.286", CLAUDE_HELP, ""))],
        &clock,
        Some(repo),
    )
    .await;
    let wire = get(server.port, "/commands/catalog").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    server.stop().await;
}

#[test]
fn routes_follow_python_matching() {
    use comandos_server::dash::native::{NativeRoute, catalog_cli::CliRoute, route};
    use http::Method;
    let cases = [
        (Method::GET, "/commands/catalog", Some(CliRoute::Catalog)),
        (Method::GET, "/commands/catalogX?y", Some(CliRoute::Catalog)),
        (Method::GET, "/chains", Some(CliRoute::ChainsGet)),
        (Method::GET, "/chains?x", None),
        (Method::POST, "/chains", Some(CliRoute::ChainsPost)),
        (Method::POST, "/chains?x", None),
        (Method::POST, "/chains/delete", None),
        (
            Method::GET,
            "/opencode/models?refresh=1",
            Some(CliRoute::OpencodeModels),
        ),
    ];
    for (method, target, want) in cases {
        let got = match route(&method, target) {
            Some(NativeRoute::Cli(r)) => Some(r),
            _ => None,
        };
        assert_eq!(got, want, "{method} {target}");
    }
}
