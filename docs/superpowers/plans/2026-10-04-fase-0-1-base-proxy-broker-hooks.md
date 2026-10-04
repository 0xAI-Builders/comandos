# Fases 0 y 1: base del workspace, proxy MCP, broker compartido y hooks — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dejar el workspace Rust limpio y con un binario único `comandos`, y sustituir en producción (de forma reversible y sin tocar sesiones vivas) el proxy MCP, el broker compartido de servidores MCP y los hooks de los agentes.

**Architecture:** Un crate binario `comandos-cli` despacha por subcomando o por el nombre con que se invoca (`argv[0]`: `cc-extensions`, `cc-notify.sh`, …), de modo que los symlinks actuales siguen funcionando. `comandos ext serve <nombre>` reemplaza al proxy Python; detrás de él, `comandos ext broker` mantiene un solo proceso por servidor MCP compartido entre sesiones por un socket Unix, con multiplexación JSON-RPC. `comandos hook <agente>` reemplaza los scripts bash/python de hooks, escribiendo exactamente los mismos archivos de estado que hoy para poder revertir.

**Tech Stack:** Rust 1.96 (edition 2024), tokio, hyper 1 (ya en el workspace), rusqlite 0.40 (ya), serde_json con `preserve_order`/`arbitrary_precision` (ya), nix 0.31. Sin dependencias nuevas salvo `regex` (sustituye al motor propio).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md`

## Global Constraints

- Ninguna tarea escribe en el servidor tmux del usuario, en `~/.claude/hooks/`, en `~/.config/comandos/`, ni reinicia `cc-dash.service`, `cc-notifyd.service`, `tmux.service` o la app GTK. Las pruebas usan `HOME` temporal y `tmux -L comandos-test`.
- Compilar siempre con `nice -n 10 cargo … -j 6` y `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-full/.build/target`.
- `cargo clippy --workspace -- -D warnings` y `cargo fmt --check` limpios antes de cada commit.
- JSON que hoy produce Python se emite byte a byte igual (usar `comandos_extensions::python_string` / `python_json`), hasta la fase de consolidación.
- Los cutovers (tareas 7 y 11) los ejecuta el controlador de la sesión, nunca un subagente, y solo con el arnés en verde.
- Español en documentación y mensajes de usuario; inglés permitido en identificadores.

## Review Focus

1. Un cliente del broker que desaparece a mitad de una llamada `tools/call`: la respuesta tardía del upstream no debe entregarse a otro cliente ni dejar el id huérfano — prueba en Tarea 9 (`late_response_after_client_gone`).
2. Dos sesiones que inician a la vez el mismo servidor MCP compartido: solo un `initialize` llega al upstream; ambas reciben el `InitializeResult` — prueba en Tarea 9 (`concurrent_initialize_single_upstream`).
3. Hook con stdin vacío o JSON truncado (Claude Code puede matar el hook por timeout): el hook sale 0 sin escribir estado parcial — prueba en Tarea 10 (`truncated_payload_writes_nothing`).
4. Servidor marcado `shared: false` (chrome-bg y demás navegadores): cada sesión debe recibir su propio upstream aunque el broker esté activo — prueba en Tarea 9 (`dedicated_server_never_shared`).
5. `cc-extensions` invocado con `python3.11 /ruta/cc-extensions serve x` por un agente antiguo (no vía symlink): el despacho por `argv[0]` no aplica; el binario debe aceptar también `comandos ext serve x` y el symlink debe seguir siendo ejecutable directo — prueba en Tarea 2 (`dispatch_by_argv0_and_explicit`).

---

### Task 1: Sacar del producto el motor regex, las tablas Unicode y la herramienta checkpoint

**Files:**
- Delete: `crates/comandos-extensions/src/output_schema.rs`, `crates/comandos-extensions/src/output_schema/` (todo), `crates/comandos-extensions/examples/checkpoint/`, `crates/comandos-extensions/examples/checkpoint_approved.rs`, `crates/comandos-extensions/examples/checkpoint_resource.rs`, `crates/comandos-extensions/examples/regex_frontend.rs`, `crates/comandos-extensions/examples/regex_frontend/`, `crates/comandos-extensions/examples/audit_unicode14.rs`, `crates/comandos-extensions/examples/generate_unicode14.rs`, `crates/comandos-extensions/tests/output_schema*.rs`, `crates/comandos-extensions/tests/output_schema*.py`, `crates/comandos-extensions/tests/output_schema*.json`
- Modify: `crates/comandos-extensions/src/lib.rs` (quitar `pub mod output_schema;`), `crates/comandos-extensions/src/main.rs` (quitar la rama `__schema_worker`), `crates/comandos-extensions/Cargo.toml` (quitar deps que solo usaba ese módulo: `jsonschema`, `num-bigint`, `num-traits`, `rustc-hash` si `cargo build` ya no las necesita)
- Modify: `docs/rust-component-inventory.md` (nota al pie: módulos retirados y por qué)

**Interfaces:**
- Consumes: nada.
- Produces: workspace que compila sin `output_schema`; `check` sigue funcionando sin validar esquemas de salida (igual que el Python actual, que no los valida).

- [ ] **Step 1: Medir antes**

Run: `cd /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-full && find crates -name '*.rs' | xargs wc -l | tail -1`
Expected: ~101 590 líneas.

- [ ] **Step 2: Borrar los archivos listados**

```bash
cd /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-full
git rm -r -q crates/comandos-extensions/src/output_schema.rs crates/comandos-extensions/src/output_schema \
  crates/comandos-extensions/examples/regex_frontend.rs crates/comandos-extensions/examples/regex_frontend \
  crates/comandos-extensions/examples/audit_unicode14.rs crates/comandos-extensions/examples/generate_unicode14.rs
git rm -q crates/comandos-extensions/tests/output_schema*.rs crates/comandos-extensions/tests/output_schema*.py crates/comandos-extensions/tests/output_schema*.json
rm -rf crates/comandos-extensions/examples/checkpoint crates/comandos-extensions/examples/checkpoint_approved.rs crates/comandos-extensions/examples/checkpoint_resource.rs
```
(Los `checkpoint*` no están en git: `rm` basta.)

- [ ] **Step 3: Quitar las referencias**

En `crates/comandos-extensions/src/lib.rs` eliminar la línea `pub mod output_schema;`. En `crates/comandos-extensions/src/main.rs` eliminar el bloque:

```rust
    if std::env::args().nth(1).as_deref() == Some("__schema_worker") {
        if args.count() == 1 {
            comandos_extensions::output_schema::worker_command();
        } else {
            println!("execution-failure");
        }
        return Ok(());
    }
```

- [ ] **Step 4: Compilar y limpiar dependencias**

Run: `nice -n 10 cargo build --workspace -j 6 2>&1 | tail -20`
Si compila, probar quitar de `crates/comandos-extensions/Cargo.toml` una a una `jsonschema`, `num-bigint`, `num-traits`, `rustc-hash` y recompilar; dejar solo las que hagan falta. Luego `cargo test --workspace -j 6 2>&1 | tail -30`.
Expected: build y tests en verde (los tests restantes de `comandos-extensions`: auth, catalog, metadata, http_client).

- [ ] **Step 5: Medir después y anotar**

Run: `find crates -name '*.rs' | xargs wc -l | tail -1`
Expected: ≈ 36 000 líneas. Añadir al final de `docs/rust-component-inventory.md`:

```markdown
## Retirado del producto el 2026-10-04

El motor de expresiones regulares CPython, las tablas Unicode 14 y la herramienta de respaldos
(`examples/checkpoint`) se retiraron: la aplicación Python no valida esquemas de salida de
herramientas y los respaldos de estado los hará el migrador de estado único. Reducción: ~65 000
líneas de Rust que no correspondían a ninguna función del inventario.
```

- [ ] **Step 6: Commit**

```bash
git add -A crates docs/rust-component-inventory.md
git commit -m "chore: retirar motor regex, tablas Unicode y herramienta checkpoint del producto"
```

---

### Task 2: Binario único `comandos` con despacho por subcomando y por `argv[0]`

**Files:**
- Create: `crates/comandos-cli/Cargo.toml`, `crates/comandos-cli/src/main.rs`, `crates/comandos-cli/src/dispatch.rs`, `crates/comandos-cli/tests/dispatch.rs`
- Modify: `Cargo.toml` (añadir `"crates/comandos-cli"` a `members`)
- Modify: `crates/comandos-extensions/src/main.rs` → mover su cuerpo a `crates/comandos-extensions/src/cli.rs` como `pub fn run(args: Vec<String>) -> Result<i32>`; `main.rs` queda llamando a `cli::run`.
- Modify: `crates/comandos-extensions/src/lib.rs` (añadir `pub mod cli;`)

**Interfaces:**
- Produces: `comandos_cli::dispatch::resolve(argv0: &str, args: &[String]) -> Command` donde

```rust
pub enum Command {
    Ext(Vec<String>),      // `comandos ext …` o argv0 = cc-extensions
    Hook(Vec<String>),     // `comandos hook …` o argv0 = cc-notify.sh | cc-usage-tool.sh | codex-notify.sh | …
    Events(Vec<String>),   // `comandos events …` (intake N1, hoy comandos-events)
    Version,
    Help,
    Unknown(String),
}
```
- Produces: `comandos_extensions::cli::run(args: Vec<String>) -> comandos_extensions::Result<i32>` (los mismos subcomandos de hoy: `import|sync|status|check|serve|count`, flags `--home`, `--catalog`).

- [ ] **Step 1: Test de despacho que falla**

`crates/comandos-cli/tests/dispatch.rs`:

```rust
use comandos_cli::dispatch::{resolve, Command};

fn v(items: &[&str]) -> Vec<String> { items.iter().map(|s| s.to_string()).collect() }

#[test]
fn dispatch_by_argv0_and_explicit() {
    assert!(matches!(resolve("/home/x/.local/bin/cc-extensions", &v(&["serve", "mobbin"])), Command::Ext(a) if a == v(&["serve", "mobbin"])));
    assert!(matches!(resolve("comandos", &v(&["ext", "serve", "mobbin"])), Command::Ext(a) if a == v(&["serve", "mobbin"])));
    assert!(matches!(resolve("/home/x/.claude/hooks/cc-notify.sh", &v(&[])), Command::Hook(a) if a == v(&["claude"])));
    assert!(matches!(resolve("cc-usage-tool.sh", &v(&[])), Command::Hook(a) if a == v(&["claude-usage"])));
    assert!(matches!(resolve("codex-notify.sh", &v(&["{}"])), Command::Hook(a) if a == v(&["codex", "{}"])));
    assert!(matches!(resolve("comandos", &v(&["--version"])), Command::Version));
    assert!(matches!(resolve("comandos", &v(&["frobnicate"])), Command::Unknown(s) if s == "frobnicate"));
}
```

- [ ] **Step 2: Correr y ver que falla**

Run: `nice -n 10 cargo test -p comandos-cli -j 6`
Expected: FAIL por crate inexistente.

- [ ] **Step 3: Crear el crate y el despacho**

`crates/comandos-cli/Cargo.toml`:

```toml
[package]
name = "comandos-cli"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "comandos"
path = "src/main.rs"

[dependencies]
comandos-extensions = { path = "../comandos-extensions" }
comandos-runtime = { path = "../comandos-runtime" }

[lints]
workspace = true
```

`crates/comandos-cli/src/dispatch.rs`:

```rust
//! Resolución del subcomando a partir del nombre invocado o del primer argumento.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Ext(Vec<String>),
    Hook(Vec<String>),
    Events(Vec<String>),
    Version,
    Help,
    Unknown(String),
}

/// Nombres heredados (symlinks `cc-*`) y el subcomando al que corresponden.
const ALIASES: &[(&str, &[&str])] = &[
    ("cc-extensions", &["ext"]),
    ("cc-notify.sh", &["hook", "claude"]),
    ("cc-usage-tool.sh", &["hook", "claude-usage"]),
    ("cc-status.sh", &["hook", "claude-status"]),
    ("codex-notify.sh", &["hook", "codex"]),
    ("codex-hooks.sh", &["hook", "codex-hooks"]),
    ("gemini-hooks.sh", &["hook", "gemini"]),
    ("agy-hooks.sh", &["hook", "agy"]),
    ("grok-hooks.py", &["hook", "grok"]),
    ("comandos-events", &["events"]),
];

pub fn resolve(argv0: &str, args: &[String]) -> Command {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    let mut words: Vec<String> = Vec::new();
    if let Some((_, prefix)) = ALIASES.iter().find(|(alias, _)| *alias == name) {
        words.extend(prefix.iter().map(|s| s.to_string()));
    }
    words.extend(args.iter().cloned());
    match words.first().map(String::as_str) {
        Some("ext") => Command::Ext(words[1..].to_vec()),
        Some("hook") => Command::Hook(words[1..].to_vec()),
        Some("events") => Command::Events(words[1..].to_vec()),
        Some("--version") | Some("version") => Command::Version,
        None | Some("--help") | Some("help") => Command::Help,
        Some(other) => Command::Unknown(other.to_string()),
    }
}
```

`crates/comandos-cli/src/main.rs`:

```rust
mod dispatch;
pub use dispatch::{resolve, Command};

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let code = match resolve(&argv[0], &argv[1..]) {
        Command::Ext(args) => comandos_extensions::cli::run(args).unwrap_or_else(|e| { eprintln!("{e}"); 1 }),
        Command::Events(args) => comandos_runtime::events_cli::run(&args).unwrap_or_else(|e| { eprintln!("{e}"); 1 }),
        Command::Hook(_) => { eprintln!("comandos hook: pendiente (Tarea 10)"); 2 }
        Command::Version => { println!("comandos {}", env!("CARGO_PKG_VERSION")); 0 }
        Command::Help => { println!("uso: comandos <ext|hook|events|--version>"); 0 }
        Command::Unknown(w) => { eprintln!("comandos: subcomando desconocido: {w}"); 2 }
    };
    std::process::exit(code);
}
```

Para que `dispatch` sea probable desde `tests/`, añadir `crates/comandos-cli/src/lib.rs` con `pub mod dispatch;` y en `main.rs` usar `comandos_cli::dispatch::{resolve, Command}` en lugar de `mod dispatch;`.

Mover el cuerpo de `crates/comandos-extensions/src/main.rs` (`fn run`, `fn catalog_command`, y todo lo demás) a `crates/comandos-extensions/src/cli.rs`, cambiando la firma a `pub fn run(args: Vec<String>) -> Result<i32>` (recibe los argumentos ya sin el nombre del programa; devuelve `Ok(0)` donde hoy hace `Ok(())`). `main.rs` queda:

```rust
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match comandos_extensions::cli::run(args) {
        Ok(code) => std::process::exit(code),
        Err(error) => { eprintln!("{error}"); std::process::exit(1); }
    }
}
```

Mover igualmente `crates/comandos-runtime/src/bin/comandos-events.rs` a `crates/comandos-runtime/src/events_cli.rs` con `pub fn run(args: &[String]) -> comandos_store::Result<i32>` (ya tiene esa forma: `fn run(args: &[String]) -> Result<i32>`), dejando el `bin` como envoltorio de dos líneas y añadiendo `pub mod events_cli;` a `crates/comandos-runtime/src/lib.rs`.

- [ ] **Step 4: Correr tests**

Run: `nice -n 10 cargo test -p comandos-cli -p comandos-extensions -p comandos-runtime -j 6`
Expected: PASS, incluidos los tests previos de extensions (que ahora prueban `cli::run`).

- [ ] **Step 5: Prueba de humo por symlink**

```bash
mkdir -p .build/bin && ln -sf "$CARGO_TARGET_DIR/debug/comandos" .build/bin/cc-extensions
.build/bin/cc-extensions --home /tmp/vacio status; echo "rc=$?"
```
Expected: mensaje de catálogo ausente (`Shared catalog missing or unsupported`), rc=1 — el mismo que da el Python sin catálogo.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/comandos-cli crates/comandos-extensions crates/comandos-runtime
git commit -m "feat(cli): binario único comandos con despacho por subcomando y argv0"
```

---

### Task 3: `xtask` con medición de RSS y arranque

**Files:**
- Create: `xtask/Cargo.toml`, `xtask/src/main.rs`, `xtask/src/rss.rs`, `.cargo/config.toml` (alias `xtask = "run -q -p xtask --"`)
- Modify: `Cargo.toml` (`members` += `"xtask"`)

**Interfaces:**
- Produces: `cargo xtask rss --samples 5 -- <cmd…>`: lanza el comando, espera 2 s, lee `/proc/<pid>/status` (VmRSS) de él y de sus hijos (vía `/proc/*/stat` ppid), imprime JSON `{"cmd":…, "rss_kib_total":…, "procs":…, "startup_ms":…}` y lo añade a `docs/verification/rss.jsonl`. Mata el árbol al terminar (SIGTERM, luego SIGKILL a 2 s).

- [ ] **Step 1: Test que falla**

`xtask/src/rss.rs` (al final):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measures_sleep_process_tree() {
        let m = measure(&["sh".into(), "-c".into(), "sleep 3 & sleep 3".into()], 1).unwrap();
        assert!(m.procs >= 2, "debe contar hijos: {m:?}");
        assert!(m.rss_kib_total > 0);
    }
}
```

- [ ] **Step 2: Correr y ver que falla**

Run: `nice -n 10 cargo test -p xtask -j 6`
Expected: FAIL (crate inexistente).

- [ ] **Step 3: Implementar**

`xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
version = "0.0.0"
edition.workspace = true
publish = false

[dependencies]
serde_json.workspace = true
nix = { version = "0.31", features = ["signal", "process"] }
```

`xtask/src/rss.rs`:

```rust
use std::{fs, process::{Command, Stdio}, thread, time::{Duration, Instant}};

#[derive(Debug)]
pub struct Measure { pub rss_kib_total: u64, pub procs: usize, pub startup_ms: u128 }

fn children(root: u32) -> Vec<u32> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            // campo 4 = ppid, después del último ')'
            let tail = stat.rsplit(')').next().unwrap_or("");
            if tail.split_whitespace().nth(1) == Some(&parent.to_string()) && !out.contains(&pid) { out.push(pid); }
        }
        i += 1;
    }
    out
}

fn rss_kib(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/status")).ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()))
        .unwrap_or(0)
}

pub fn measure(cmd: &[String], settle_secs: u64) -> Result<Measure, String> {
    let start = Instant::now();
    let mut child = Command::new(&cmd[0]).args(&cmd[1..]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map_err(|e| format!("{}: {e}", cmd[0]))?;
    thread::sleep(Duration::from_secs(settle_secs));
    let startup_ms = start.elapsed().as_millis();
    let pids = children(child.id());
    let rss_kib_total = pids.iter().map(|p| rss_kib(*p)).sum();
    let procs = pids.len();
    for pid in pids.iter().rev() {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(*pid as i32), nix::sys::signal::Signal::SIGTERM);
    }
    thread::sleep(Duration::from_secs(2));
    for pid in pids.iter().rev() {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(*pid as i32), nix::sys::signal::Signal::SIGKILL);
    }
    let _ = child.wait();
    Ok(Measure { rss_kib_total, procs, startup_ms })
}
```

`xtask/src/main.rs`:

```rust
mod rss;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("rss") => {
            let sep = args.iter().position(|a| a == "--").expect("uso: xtask rss [--samples N] -- <cmd…>");
            let samples: usize = args[1..sep].windows(2).find(|w| w[0] == "--samples").and_then(|w| w[1].parse().ok()).unwrap_or(1);
            let cmd = &args[sep + 1..];
            let mut best = u64::MAX;
            let mut last = None;
            for _ in 0..samples { let m = rss::measure(cmd, 2).expect("medición"); best = best.min(m.rss_kib_total); last = Some(m); }
            let m = last.unwrap();
            let line = serde_json::json!({"cmd": cmd, "rss_kib_total": m.rss_kib_total, "rss_kib_min": best, "procs": m.procs, "startup_ms": m.startup_ms, "date": chrono_free_now()});
            println!("{line}");
            std::fs::create_dir_all("docs/verification").unwrap();
            use std::io::Write;
            writeln!(std::fs::OpenOptions::new().append(true).create(true).open("docs/verification/rss.jsonl").unwrap(), "{line}").unwrap();
        }
        _ => eprintln!("subcomandos: rss"),
    }
}
fn chrono_free_now() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    s.to_string()
}
```

`.cargo/config.toml`:

```toml
[alias]
xtask = "run -q -p xtask --"
```

- [ ] **Step 4: Correr tests**

Run: `nice -n 10 cargo test -p xtask -j 6`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock xtask .cargo/config.toml
git commit -m "chore(xtask): medición de RSS y arranque de árboles de procesos"
```

---

### Task 4: Paridad del proxy HTTP (`comandos ext serve`) contra el Python con un upstream falso

**Files:**
- Create: `crates/comandos-extensions/tests/serve_parity.rs`, `crates/comandos-extensions/tests/support/fake_mcp_http.rs` (servidor HTTP falso en Rust con `hyper`, arrancado dentro del test en un hilo con `tokio`)
- Test (oráculo): `bin/cc-extensions serve <nombre>` (Python, del worktree) con `HOME` temporal.

**Interfaces:**
- Consumes: `comandos ext serve <nombre>` (Tarea 2); `catalog.json` mínimo en `HOME/.config/comandos/extensions/catalog.json` con un servidor `{"enabled":true,"url":"http://127.0.0.1:<puerto>/mcp","transport":"http"}`.
- Produces: evidencia de paridad en `docs/verification/serve-parity.md`.

- [ ] **Step 1: Upstream falso**

`crates/comandos-extensions/tests/support/fake_mcp_http.rs`: `pub fn spawn(port: u16, log: Arc<Mutex<Vec<String>>>) -> std::thread::JoinHandle<()>` que levanta con `hyper` un servidor `streamable-http` mínimo (POST `/mcp`, respuesta JSON única) que responde a `initialize` (`{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}`), `tools/list` (dos herramientas: `echo` con `inputSchema {"type":"object","properties":{"text":{"type":"string"}}}` y `fail`), `tools/call` (`echo` devuelve `[{"type":"text","text":<text>}]`; `fail` devuelve `isError:true`), y a cualquier otro método con error `-32601`. Registra cada petición recibida en `log`.

- [ ] **Step 2: Test de paridad que falla**

`crates/comandos-extensions/tests/serve_parity.rs`:

```rust
//! Compara byte a byte las respuestas del proxy Rust y del proxy Python ante la misma
//! secuencia JSON-RPC por stdio, con un upstream HTTP falso.
mod support;
use std::{fs, io::{BufRead, BufReader, Write}, net::TcpListener, path::PathBuf, process::{Command, Stdio}, thread, time::Duration};

const SEQUENCE: &[&str] = &[
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hola ñ 日本"}}}"#,
    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"fail","arguments":{}}}"#,
    r#"{"jsonrpc":"2.0","id":5,"method":"prompts/list"}"#,
];

fn free_port() -> u16 { TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port() }

fn run_proxy(cmd: &mut Command, home: &PathBuf) -> Vec<String> {
    let mut child = cmd.env("HOME", home).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let out = BufReader::new(child.stdout.take().unwrap());
    let mut lines = Vec::new();
    let mut reader = out.lines();
    for msg in SEQUENCE {
        writeln!(stdin, "{msg}").unwrap();
        if !msg.contains("notifications/") { lines.push(reader.next().unwrap().unwrap()); }
    }
    drop(stdin);
    let _ = child.wait();
    lines
}

#[test]
fn rust_proxy_matches_python_proxy() {
    let port = free_port();
    let home = std::env::temp_dir().join(format!("comandos-parity-{port}"));
    fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
    fs::write(home.join(".config/comandos/extensions/catalog.json"),
        format!(r#"{{"version":1,"servers":{{"fake":{{"enabled":true,"url":"http://127.0.0.1:{port}/mcp","transport":"http"}}}}}}"#)).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let _fake = support::fake_mcp_http::spawn(port, log.clone());
    thread::sleep(Duration::from_millis(200));

    let rust = run_proxy(Command::new(env!("CARGO_BIN_EXE_comandos-extensions")).args(["serve", "fake"]), &home);
    let python = run_proxy(Command::new("python3.11").arg(root.join("bin/cc-extensions")).args(["serve", "fake"]), &home);
    assert_eq!(rust, python, "las respuestas del proxy difieren");
}
```

- [ ] **Step 3: Correr**

Run: `nice -n 10 cargo test -p comandos-extensions --test serve_parity -j 6 -- --nocapture`
Expected: puede fallar en orden de claves, espacios o en el error `-32601`. Corregir en `crates/comandos-extensions/src/serve.rs` hasta igualar (usar `python_string` para la serialización; conservar el orden de claves del upstream con `preserve_order`). Si el Python necesita el venv (`mcp`), el fallback a `~/.local/share/comandos/extensions-venv` ya está en `bin/cc-extensions`.

- [ ] **Step 4: Medir RSS de ambos**

```bash
cargo xtask rss --samples 3 -- python3.11 bin/cc-extensions serve fake      # con HOME y fake levantados igual que en el test
cargo xtask rss --samples 3 -- $CARGO_TARGET_DIR/release/comandos ext serve fake
```
(compilar release antes: `nice -n 10 cargo build --release -p comandos-cli -j 6`). Anotar en `docs/verification/serve-parity.md`: secuencia probada, resultado, RSS Python vs Rust.

- [ ] **Step 5: Commit**

```bash
git add crates/comandos-extensions docs/verification
git commit -m "test(ext): paridad byte a byte del proxy MCP HTTP con el proxy Python y medición de RSS"
```

---

### Task 5: Paridad del proxy stdio (exec directo) y de `count|status|sync|check`

**Files:**
- Create: `crates/comandos-extensions/tests/serve_stdio_parity.rs`
- Modify (solo si hay diferencias): `crates/comandos-extensions/src/cli.rs`, `src/serve.rs`, `src/check.rs`

**Interfaces:**
- Consumes: Tarea 2.
- Produces: confirmación de que `serve <stdio>` hace `exec` del comando del catálogo con `env` resuelto (igual que el Python: `os.execvpe(command,[command,*args],resolved_env(spec))`, `cwd` aplicado, stderr a `/dev/null`).

- [ ] **Step 1: Test que falla**

```rust
//! `serve` de un servidor stdio debe reemplazar el proceso por el comando del catálogo.
use std::{fs, path::PathBuf, process::Command};

#[test]
fn stdio_serve_execs_catalog_command_with_env() {
    let home = std::env::temp_dir().join("comandos-stdio-parity");
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
    fs::write(home.join(".config/comandos/extensions/catalog.json"),
      r#"{"version":1,"servers":{"eco":{"enabled":true,"command":"sh","args":["-c","printf '%s|%s' \"$MARK\" \"$PWD\""],"env":{"MARK":"${HOME}/m"},"cwd":"/tmp"}}}"#).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_comandos-extensions")).args(["serve", "eco"]).env("HOME", &home).output().unwrap();
    let expected = format!("{}/m|/tmp", home.display());
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
    let py = Command::new("python3.11").arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-extensions")).args(["serve", "eco"]).env("HOME", &home).output().unwrap();
    assert_eq!(out.stdout, py.stdout, "Rust y Python deben ejecutar el mismo comando con el mismo entorno");
}

#[test]
fn status_and_count_match_python() {
    let home = std::env::temp_dir().join("comandos-stdio-parity");
    for args in [vec!["status"], vec!["count"]] {
        let r = Command::new(env!("CARGO_BIN_EXE_comandos-extensions")).args(&args).env("HOME", &home).output().unwrap();
        let p = Command::new("python3.11").arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-extensions")).args(&args).env("HOME", &home).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&p.stdout), "{args:?}");
    }
}
```
(`count` no existe en el Python: si Python devuelve error, el test solo exige que el Rust produzca el JSON `{"servers":N,"enabled":M}`; ajustar la aserción a `status` para la comparación y documentar `count` como extensión Rust ya revisada.)

- [ ] **Step 2: Correr, corregir hasta pasar, commit**

Run: `nice -n 10 cargo test -p comandos-extensions --test serve_stdio_parity -j 6`
Expected: PASS. Commit: `git commit -am "test(ext): paridad de serve stdio, status y count con el proxy Python"`.

---

### Task 6: Instalación en paralelo del binario (sin cutover)

**Files:**
- Create: `crates/comandos-cli/src/install.rs` (subcomando `comandos install --stage` y `--rollback <nombre>`), test `crates/comandos-cli/tests/install.rs`
- Modify: `crates/comandos-cli/src/dispatch.rs` (añadir `Install(Vec<String>)`), `src/main.rs`

**Interfaces:**
- Produces: `comandos install --stage` copia el binario release a `~/.local/share/comandos/bin/comandos` (atómico: escribir `.tmp` + `rename`) sin tocar ningún symlink. `comandos install --link <nombre>` cambia `~/.local/bin/<nombre>` (o `~/.claude/hooks/<nombre>` para hooks) a un symlink al binario, guardando el destino anterior en `~/.local/share/comandos/rollback/<nombre>.target`. `comandos install --rollback <nombre>` restaura ese destino. Todo con `HOME` sobreescribible por `--home` para pruebas.

- [ ] **Step 1: Test que falla**

```rust
use std::{fs, os::unix::fs::symlink, process::Command};

#[test]
fn link_and_rollback_round_trip() {
    let home = std::env::temp_dir().join("comandos-install-test");
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    fs::write(home.join("old-target"), "#!/bin/sh\necho old\n").unwrap();
    symlink(home.join("old-target"), home.join(".local/bin/cc-extensions")).unwrap();
    let bin = env!("CARGO_BIN_EXE_comandos");
    assert!(Command::new(bin).args(["install", "--home", home.to_str().unwrap(), "--stage"]).status().unwrap().success());
    assert!(home.join(".local/share/comandos/bin/comandos").exists());
    assert!(Command::new(bin).args(["install", "--home", home.to_str().unwrap(), "--link", "cc-extensions"]).status().unwrap().success());
    assert_eq!(fs::read_link(home.join(".local/bin/cc-extensions")).unwrap(), home.join(".local/share/comandos/bin/comandos"));
    assert_eq!(fs::read_to_string(home.join(".local/share/comandos/rollback/cc-extensions.target")).unwrap().trim(), home.join("old-target").to_str().unwrap());
    assert!(Command::new(bin).args(["install", "--home", home.to_str().unwrap(), "--rollback", "cc-extensions"]).status().unwrap().success());
    assert_eq!(fs::read_link(home.join(".local/bin/cc-extensions")).unwrap(), home.join("old-target"));
}
```

- [ ] **Step 2: Implementar `install.rs`**

```rust
use std::{fs, os::unix::fs::symlink, path::{Path, PathBuf}};

const HOOK_NAMES: &[&str] = &["cc-notify.sh", "cc-status.sh", "cc-usage-tool.sh"];

pub fn run(args: &[String]) -> Result<i32, String> {
    let mut home = std::env::var("HOME").map(PathBuf::from).map_err(|_| "HOME no definido")?;
    let mut i = 0;
    let mut action: Option<(&str, Option<String>)> = None;
    while i < args.len() {
        match args[i].as_str() {
            "--home" => { home = PathBuf::from(args.get(i + 1).ok_or("--home requiere ruta")?); i += 2; }
            "--stage" => { action = Some(("stage", None)); i += 1; }
            "--link" | "--rollback" => { action = Some((args[i].trim_start_matches("--"), Some(args.get(i + 1).ok_or("falta nombre")?.clone()))); i += 2; }
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    let staged = home.join(".local/share/comandos/bin/comandos");
    match action {
        Some(("stage", _)) => stage(&staged),
        Some(("link", Some(name))) => link(&home, &staged, &name),
        Some(("rollback", Some(name))) => rollback(&home, &name),
        _ => Err("uso: comandos install [--home DIR] (--stage | --link NOMBRE | --rollback NOMBRE)".into()),
    }?;
    Ok(0)
}

fn link_path(home: &Path, name: &str) -> PathBuf {
    if HOOK_NAMES.contains(&name) { home.join(".claude/hooks").join(name) } else { home.join(".local/bin").join(name) }
}

fn stage(staged: &Path) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    fs::create_dir_all(staged.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = staged.with_extension("tmp");
    fs::copy(&me, &tmp).map_err(|e| e.to_string())?;
    fs::rename(&tmp, staged).map_err(|e| e.to_string())
}

fn link(home: &Path, staged: &Path, name: &str) -> Result<(), String> {
    if !staged.exists() { return Err("primero: comandos install --stage".into()); }
    let path = link_path(home, name);
    let rollback_dir = home.join(".local/share/comandos/rollback");
    fs::create_dir_all(&rollback_dir).map_err(|e| e.to_string())?;
    let previous = fs::read_link(&path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    fs::write(rollback_dir.join(format!("{name}.target")), format!("{previous}\n")).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("comandos-tmp");
    let _ = fs::remove_file(&tmp);
    symlink(staged, &tmp).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn rollback(home: &Path, name: &str) -> Result<(), String> {
    let target = fs::read_to_string(home.join(".local/share/comandos/rollback").join(format!("{name}.target"))).map_err(|_| format!("sin registro de rollback para {name}"))?;
    let target = target.trim();
    if target.is_empty() { return Err("el destino anterior estaba vacío; restaurar a mano".into()); }
    let path = link_path(home, name);
    let tmp = path.with_extension("comandos-tmp");
    let _ = fs::remove_file(&tmp);
    symlink(target, &tmp).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}
```

Añadir en `dispatch.rs` la variante `Install(Vec<String>)` para `Some("install")` y en `main.rs` la rama `Command::Install(a) => comandos_cli::install::run(&a).unwrap_or_else(|e| { eprintln!("{e}"); 1 })`.

- [ ] **Step 3: Correr tests y commit**

Run: `nice -n 10 cargo test -p comandos-cli -j 6`
Expected: PASS. `git add crates/comandos-cli && git commit -m "feat(cli): comandos install --stage/--link/--rollback con symlinks atómicos"`.

---

### Task 7 (controlador): Cutover del proxy MCP para sesiones nuevas

Prerrequisitos: Tareas 4 y 5 en verde; release compilado; medición de RSS anotada.

- [ ] **Step 1**: `nice -n 10 cargo build --release -p comandos-cli -j 6 && $CARGO_TARGET_DIR/release/comandos install --stage`.
- [ ] **Step 2**: Prueba real sin cutover: `HOME=$HOME ~/.local/share/comandos/bin/comandos ext serve mobbin` alimentado con la secuencia `initialize` + `tools/list` del test de la Tarea 4 (por `printf | …`); comparar con `python3.11 bin/cc-extensions serve mobbin` igual. Debe coincidir (mismo catálogo y credenciales reales, solo lectura).
- [ ] **Step 3**: `~/.local/share/comandos/bin/comandos install --link cc-extensions`. Verificar `readlink ~/.local/bin/cc-extensions`. A partir de aquí toda sesión nueva de Claude/Codex usa el proxy Rust; las 232 instancias Python vivas siguen hasta que su sesión termine.
- [ ] **Step 4**: Abrir una sesión de prueba (`claude -p 'di hola' --max-turns 1` en un directorio de prueba) y comprobar en `ps` que sus MCP son `comandos ext serve …` y que `~/.claude/debug` no registra fallos de MCP.
- [ ] **Step 5**: Reversión documentada: `~/.local/share/comandos/bin/comandos install --rollback cc-extensions`. Anotar en `docs/verification/cutover-ext.md` fecha, RSS por proceso antes/después y el comando de vuelta.

---

### Task 8: Protocolo del broker: multiplexación JSON-RPC pura

**Files:**
- Create: `crates/comandos-extensions/src/broker/mod.rs`, `crates/comandos-extensions/src/broker/mux.rs`, `crates/comandos-extensions/tests/broker_mux.rs`
- Modify: `crates/comandos-extensions/src/lib.rs` (`pub mod broker;`)

**Interfaces:**
- Produces (sin E/S, probable en aislamiento):

```rust
pub type ClientId = u32;
pub struct Mux { /* privado */ }
pub enum Outbound { ToUpstream(Vec<u8>), ToClient(ClientId, Vec<u8>) }
impl Mux {
    pub fn new() -> Self;
    pub fn add_client(&mut self) -> ClientId;
    pub fn remove_client(&mut self, id: ClientId) -> Vec<Outbound>;          // cancela sus requests pendientes
    pub fn from_client(&mut self, id: ClientId, line: &[u8]) -> Vec<Outbound>;
    pub fn from_upstream(&mut self, line: &[u8]) -> Vec<Outbound>;
    pub fn upstream_initialized(&self) -> bool;
}
```
Reglas: (1) el primer `initialize` de un cliente se reenvía al upstream; los siguientes reciben el `InitializeResult` cacheado sin tocar el upstream, y su `notifications/initialized` se traga; (2) cada request de cliente recibe un id upstream `u64` nuevo; la respuesta se traduce al id original (número o string) y se entrega solo a ese cliente; (3) notificaciones del upstream se difunden a todos los clientes inicializados; (4) requests del upstream (`roots/list`, `sampling/createMessage`, `elicitation/create`) se envían al cliente más reciente con capacidad para ello según su `initialize`; su respuesta se traduce de vuelta; (5) si un cliente se va, sus requests pendientes se marcan huérfanos y la respuesta tardía se descarta.

- [ ] **Step 1: Tests que fallan**

`crates/comandos-extensions/tests/broker_mux.rs`:

```rust
use comandos_extensions::broker::mux::{Mux, Outbound};
use serde_json::{json, Value};

fn j(v: Value) -> Vec<u8> { serde_json::to_vec(&v).unwrap() }
fn parse(b: &[u8]) -> Value { serde_json::from_slice(b).unwrap() }
fn init(id: u64) -> Vec<u8> { j(json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"1"}}})) }
fn init_result(id: Value) -> Vec<u8> { j(json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"up","version":"1"}}})) }

#[test]
fn concurrent_initialize_single_upstream() {
    let mut m = Mux::new();
    let a = m.add_client(); let b = m.add_client();
    let out_a = m.from_client(a, &init(1));
    assert!(matches!(out_a.as_slice(), [Outbound::ToUpstream(_)]));
    let out_b = m.from_client(b, &init(7));
    assert!(out_b.is_empty(), "el segundo initialize espera al upstream");
    let up_id = parse(match &out_a[0] { Outbound::ToUpstream(l) => l, _ => unreachable!() })["id"].clone();
    let out = m.from_upstream(&init_result(up_id));
    let ids: Vec<(u32, Value)> = out.iter().map(|o| match o { Outbound::ToClient(c, l) => (*c, parse(l)["id"].clone()), _ => panic!() }).collect();
    assert_eq!(ids, vec![(a, json!(1)), (b, json!(7))]);
    assert!(m.upstream_initialized());
    assert!(m.from_client(a, &j(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))).len() == 1);
    assert!(m.from_client(b, &j(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))).is_empty());
}

#[test]
fn responses_route_back_with_original_ids_including_strings() {
    let mut m = Mux::new();
    let a = m.add_client(); let b = m.add_client();
    let _ = m.from_client(a, &init(1)); let up = m.from_upstream(&init_result(json!(1)));
    let _ = up; let _ = m.from_client(b, &init(1));
    let oa = m.from_client(a, &j(json!({"jsonrpc":"2.0","id":"x-1","method":"tools/list"})));
    let ob = m.from_client(b, &j(json!({"jsonrpc":"2.0","id":"x-1","method":"tools/list"})));
    let ida = parse(match &oa[0] { Outbound::ToUpstream(l) => l, _ => panic!() })["id"].clone();
    let idb = parse(match &ob[0] { Outbound::ToUpstream(l) => l, _ => panic!() })["id"].clone();
    assert_ne!(ida, idb);
    let r = m.from_upstream(&j(json!({"jsonrpc":"2.0","id":idb,"result":{"tools":[]}})));
    assert!(matches!(r.as_slice(), [Outbound::ToClient(c, l)] if *c == b && parse(l)["id"] == json!("x-1")));
}

#[test]
fn late_response_after_client_gone() {
    let mut m = Mux::new();
    let a = m.add_client();
    let _ = m.from_client(a, &init(1)); let _ = m.from_upstream(&init_result(json!(1)));
    let o = m.from_client(a, &j(json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"x","arguments":{}}})));
    let up_id = parse(match &o[0] { Outbound::ToUpstream(l) => l, _ => panic!() })["id"].clone();
    let _ = m.remove_client(a);
    let late = m.from_upstream(&j(json!({"jsonrpc":"2.0","id":up_id,"result":{"content":[]}})));
    assert!(late.is_empty(), "la respuesta tardía no debe ir a nadie");
}

#[test]
fn upstream_notifications_broadcast_only_to_initialized_clients() {
    let mut m = Mux::new();
    let a = m.add_client(); let _b = m.add_client();
    let _ = m.from_client(a, &init(1)); let _ = m.from_upstream(&init_result(json!(1)));
    let out = m.from_upstream(&j(json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"})));
    assert_eq!(out.len(), 1);
    assert!(matches!(&out[0], Outbound::ToClient(c, _) if *c == a));
}
```

- [ ] **Step 2: Correr y ver que falla**

Run: `nice -n 10 cargo test -p comandos-extensions --test broker_mux -j 6`
Expected: FAIL (módulo inexistente).

- [ ] **Step 3: Implementar `mux.rs`**

```rust
//! Multiplexación JSON-RPC de varios clientes MCP sobre un único upstream. Sin E/S.
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};

pub type ClientId = u32;

#[derive(Debug, PartialEq, Eq)]
pub enum Outbound { ToUpstream(Vec<u8>), ToClient(ClientId, Vec<u8>) }

#[derive(Default)]
struct Client { initialized: bool, caps: Value }

#[derive(Default)]
pub struct Mux {
    next_client: ClientId,
    next_upstream_id: u64,
    clients: BTreeMap<ClientId, Client>,
    pending: HashMap<u64, (ClientId, Value)>,          // id upstream -> (cliente, id original)
    waiting_init: Vec<(ClientId, Value)>,              // clientes cuyo initialize espera la primera respuesta
    init_result: Option<Value>,
    init_inflight: bool,
    upstream_requests: HashMap<Value, (ClientId, Value)>, // id upstream original -> (cliente, id que le dimos)
}

impl Mux {
    pub fn new() -> Self { Self::default() }
    pub fn upstream_initialized(&self) -> bool { self.init_result.is_some() }
    pub fn add_client(&mut self) -> ClientId { self.next_client += 1; self.clients.insert(self.next_client, Client::default()); self.next_client }

    pub fn remove_client(&mut self, id: ClientId) -> Vec<Outbound> {
        self.clients.remove(&id);
        self.pending.retain(|_, (c, _)| *c != id);
        self.waiting_init.retain(|(c, _)| *c != id);
        Vec::new()
    }

    pub fn from_client(&mut self, id: ClientId, line: &[u8]) -> Vec<Outbound> {
        let Ok(Value::Object(mut msg)) = serde_json::from_slice::<Value>(line) else { return Vec::new() };
        let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
        match method.as_deref() {
            Some("initialize") => {
                let orig = msg.get("id").cloned().unwrap_or(Value::Null);
                if let Some(c) = self.clients.get_mut(&id) { c.caps = msg.get("params").and_then(|p| p.get("capabilities")).cloned().unwrap_or(Value::Null); }
                if let Some(result) = &self.init_result {
                    return vec![Outbound::ToClient(id, response(orig, result.clone()))];
                }
                self.waiting_init.push((id, orig));
                if self.init_inflight { return Vec::new(); }
                self.init_inflight = true;
                let up = self.alloc(id, Value::Null);
                msg.insert("id".into(), Value::from(up));
                vec![Outbound::ToUpstream(bytes(msg))]
            }
            Some("notifications/initialized") => {
                let first = !self.clients.values().any(|c| c.initialized);
                if let Some(c) = self.clients.get_mut(&id) { c.initialized = true; }
                if first { vec![Outbound::ToUpstream(bytes(msg))] } else { Vec::new() }
            }
            Some(_) if msg.contains_key("id") => {
                let orig = msg["id"].clone();
                let up = self.alloc(id, orig);
                msg.insert("id".into(), Value::from(up));
                vec![Outbound::ToUpstream(bytes(msg))]
            }
            Some(_) => vec![Outbound::ToUpstream(bytes(msg))],
            None => {
                // respuesta del cliente a un request del upstream
                let Some(given) = msg.get("id").cloned() else { return Vec::new() };
                let Some((orig_id, _)) = self.upstream_requests.iter().find(|(_, (c, g))| *c == id && *g == given).map(|(k, v)| (k.clone(), v.clone())) else { return Vec::new() };
                self.upstream_requests.remove(&orig_id);
                msg.insert("id".into(), orig_id);
                vec![Outbound::ToUpstream(bytes(msg))]
            }
        }
    }

    pub fn from_upstream(&mut self, line: &[u8]) -> Vec<Outbound> {
        let Ok(Value::Object(mut msg)) = serde_json::from_slice::<Value>(line) else { return Vec::new() };
        if msg.contains_key("method") {
            if msg.contains_key("id") {
                // request del upstream: al cliente inicializado más reciente
                let Some((&target, _)) = self.clients.iter().rev().find(|(_, c)| c.initialized) else { return Vec::new() };
                let orig = msg["id"].clone();
                let given = Value::from(self.next_id());
                self.upstream_requests.insert(orig, (target, given.clone()));
                msg.insert("id".into(), given);
                return vec![Outbound::ToClient(target, bytes(msg))];
            }
            let b = bytes(msg);
            return self.clients.iter().filter(|(_, c)| c.initialized).map(|(id, _)| Outbound::ToClient(*id, b.clone())).collect();
        }
        let Some(up) = msg.get("id").and_then(Value::as_u64) else { return Vec::new() };
        let Some((client, orig)) = self.pending.remove(&up) else { return Vec::new() };
        if self.init_inflight && orig.is_null() {
            self.init_inflight = false;
            if let Some(result) = msg.get("result").cloned() {
                self.init_result = Some(result.clone());
                let waiting = std::mem::take(&mut self.waiting_init);
                return waiting.into_iter().filter(|(c, _)| self.clients.contains_key(c))
                    .map(|(c, oid)| Outbound::ToClient(c, response(oid, result.clone()))).collect();
            }
            let waiting = std::mem::take(&mut self.waiting_init);
            let err = msg.get("error").cloned().unwrap_or(Value::Null);
            return waiting.into_iter().map(|(c, oid)| Outbound::ToClient(c, error_response(oid, err.clone()))).collect();
        }
        if !self.clients.contains_key(&client) { return Vec::new(); }
        msg.insert("id".into(), orig);
        vec![Outbound::ToClient(client, bytes(msg))]
    }

    fn next_id(&mut self) -> u64 { self.next_upstream_id += 1; self.next_upstream_id }
    fn alloc(&mut self, client: ClientId, orig: Value) -> u64 { let id = self.next_id(); self.pending.insert(id, (client, orig)); id }
}

fn bytes(m: Map<String, Value>) -> Vec<u8> { serde_json::to_vec(&Value::Object(m)).unwrap_or_default() }
fn response(id: Value, result: Value) -> Vec<u8> { let mut m = Map::new(); m.insert("jsonrpc".into(), "2.0".into()); m.insert("id".into(), id); m.insert("result".into(), result); bytes(m) }
fn error_response(id: Value, error: Value) -> Vec<u8> { let mut m = Map::new(); m.insert("jsonrpc".into(), "2.0".into()); m.insert("id".into(), id); m.insert("error".into(), error); bytes(m) }
```

`crates/comandos-extensions/src/broker/mod.rs`: `pub mod mux;`.

- [ ] **Step 4: Correr tests hasta verde; clippy; commit**

Run: `nice -n 10 cargo test -p comandos-extensions --test broker_mux -j 6 && cargo clippy -p comandos-extensions -- -D warnings`
Commit: `git add crates/comandos-extensions && git commit -m "feat(ext): multiplexor JSON-RPC puro para el broker MCP compartido"`.

---

### Task 9: Daemon del broker y cliente fino sobre socket Unix

**Files:**
- Create: `crates/comandos-extensions/src/broker/daemon.rs`, `crates/comandos-extensions/src/broker/client.rs`, `crates/comandos-extensions/src/broker/upstream.rs`, `crates/comandos-extensions/tests/broker_daemon.rs`, `crates/comandos-extensions/src/bin/fake_mcp_stdio.rs` (binario de prueba de 25 líneas: lee líneas JSON de stdin y responde a las que traen `id` con `{"jsonrpc":"2.0","id":<id>,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"eco","version":"<pid>"}}}`; declarado en `Cargo.toml` con `required-features = []` y usado solo en tests), `systemd/comandos-broker.service`
- Modify: `crates/comandos-extensions/src/cli.rs` (subcomando `broker`; `serve` intenta primero el broker si el servidor es compartible), `crates/comandos-extensions/src/config.rs` (leer `shared` del catálogo; por defecto `true`, salvo `DEDICATED` = `["chrome-bg","claude-in-chrome","playwright","x-playwright","lightpanda","obscura","screenwright","teams","claude-codex"]` → `false`)

**Interfaces:**
- Produces: socket `$XDG_RUNTIME_DIR/comandos/broker.sock` (o `/tmp/comandos-<uid>/broker.sock`). Protocolo: el cliente envía una primera línea `{"attach":"<nombre>"}`; el daemon responde `{"ok":true}` o `{"error":"…"}`; desde ahí, líneas JSON-RPC en ambos sentidos tal cual (el daemon aplica `Mux` por nombre). Un upstream por nombre: `upstream.rs` lanza el comando stdio del catálogo (con `command(spec,true)` ya existente) o abre la sesión HTTP (reutilizando `serve.rs`), y lo cierra 600 s después de que el último cliente se desconecte (configurable con `COMANDOS_BROKER_IDLE_SECS`). Si el upstream muere, todos sus clientes reciben EOF y el siguiente `attach` lo relanza.
- `comandos ext serve <nombre>`: si `shared` y el socket responde en 300 ms, actúa como cliente fino (`client.rs`: copia stdin→socket y socket→stdout con `tokio::io::copy_bidirectional`); si no, comportamiento actual (proxy directo). Nunca inicia el daemon por sí mismo: lo hace systemd.
- `systemd/comandos-broker.service`:

```ini
[Unit]
Description=ComandOS — broker compartido de servidores MCP
After=default.target

[Service]
ExecStart=%h/.local/share/comandos/bin/comandos ext broker
Restart=on-failure
RestartSec=2
MemoryMax=4G
ManagedOOMPreference=avoid

[Install]
WantedBy=default.target
```

- [ ] **Step 1: Tests de integración que fallan**

`crates/comandos-extensions/tests/broker_daemon.rs` (usa el binario de prueba `fake_mcp_stdio` como upstream compartido y un `sh -c` como servidor dedicado):

```rust
//! Dos clientes finos contra un daemon con un upstream compartido.
use std::{fs, io::{BufRead, BufReader, Write}, process::{Command, Stdio}, thread, time::Duration};

fn home_with(name: &str, server: &str) -> std::path::PathBuf {
    let home = std::env::temp_dir().join(format!("comandos-broker-{name}"));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
    fs::write(home.join(".config/comandos/extensions/catalog.json"), format!(r#"{{"version":1,"servers":{{"eco":{server},"solo":{{"enabled":true,"shared":false,"command":"sh","args":["-c","echo $$"]}}}}}}"#)).unwrap();
    home
}

#[test]
fn two_clients_share_one_upstream_and_dedicated_is_not_shared() {
    // upstream stdio de prueba (ejemplo Rust `fake_mcp_stdio`, compilado por cargo para los tests):
    // responde a cada request con un InitializeResult cuyo serverInfo.version es su propio pid.
    let server = format!(r#"{{"enabled":true,"command":"{}"}}"#, env!("CARGO_BIN_EXE_fake_mcp_stdio"));
    let home = home_with("share", server);
    let runtime = home.join("run"); fs::create_dir_all(&runtime).unwrap();
    let bin = env!("CARGO_BIN_EXE_comandos-extensions");
    let mut daemon = Command::new(bin).arg("broker").env("HOME", &home).env("XDG_RUNTIME_DIR", &runtime).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
    thread::sleep(Duration::from_millis(400));
    let ask = |args: &[&str]| {
        let mut c = Command::new(bin).args(args).env("HOME", &home).env("XDG_RUNTIME_DIR", &runtime).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut i = c.stdin.take().unwrap();
        writeln!(i, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"t","version":"1"}}}}}}"#).unwrap();
        let line = BufReader::new(c.stdout.take().unwrap()).lines().next().unwrap().unwrap();
        drop(i); let _ = c.wait();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["result"]["serverInfo"]["version"].as_str().unwrap().to_string()
    };
    let p1 = ask(&["serve", "eco"]); let p2 = ask(&["serve", "eco"]);
    assert_eq!(p1, p2, "ambos clientes deben hablar con el mismo proceso upstream");
    // dedicado: `solo` imprime su pid y sale; dos invocaciones ⇒ pids distintos
    let s1 = Command::new(bin).args(["serve", "solo"]).env("HOME", &home).env("XDG_RUNTIME_DIR", &runtime).output().unwrap();
    let s2 = Command::new(bin).args(["serve", "solo"]).env("HOME", &home).env("XDG_RUNTIME_DIR", &runtime).output().unwrap();
    assert_ne!(s1.stdout, s2.stdout, "dedicated_server_never_shared");
    let _ = daemon.kill();
}

#[test]
fn serve_falls_back_to_direct_proxy_without_daemon() {
    let home = home_with("nodaemon", r#"{"enabled":true,"command":"sh","args":["-c","echo direct"]}"#);
    let out = Command::new(env!("CARGO_BIN_EXE_comandos-extensions")).args(["serve", "eco"]).env("HOME", &home).env("XDG_RUNTIME_DIR", home.join("none")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "direct");
}
```

- [ ] **Step 2: Correr y ver que falla**

Run: `nice -n 10 cargo test -p comandos-extensions --test broker_daemon -j 6`

- [ ] **Step 3: Implementar**

`upstream.rs`: `pub struct Upstream { tx: mpsc::Sender<Vec<u8>>, pub mux: Mux }` con `pub async fn spawn(spec: &Value, home: &Path, on_line: mpsc::Sender<Vec<u8>>) -> Result<Upstream>`: si `spec["command"]` existe, `tokio::process::Command` con `command(spec, true)` (ya existe en `lib.rs`), `stdin/stdout` piped, `stderr` null; una tarea lee líneas de stdout y las manda por `on_line`; `tx` escribe en stdin. Si es HTTP, reutilizar el cliente de `serve.rs` (extraer de `serve()` una función `pub async fn http_session(home, name, spec) -> Result<(mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)>`).

`daemon.rs`: `pub async fn run(home: &Path) -> Result<()>`: lee catálogo; crea el socket (borra el anterior si no responde a `connect`); por cada conexión, lee la línea `attach`, valida nombre y `shared`, obtiene o crea el `Upstream` del nombre (un `HashMap<String, Arc<Mutex<Shared>>>` con `mux`, `clients` y `idle_since`), responde `{"ok":true}`, registra `ClientId = mux.add_client()` y entra en el bucle: líneas del cliente → `mux.from_client` → despacho de `Outbound`; líneas del upstream (canal broadcast por nombre) → `mux.from_upstream` → despacho. Al cerrar el cliente, `remove_client`; si quedan 0, programar cierre del upstream tras `idle_secs`. Si el upstream da EOF: cerrar todas las conexiones de ese nombre y eliminarlo del mapa.

`client.rs`: `pub async fn attach(socket: &Path, name: &str) -> Result<bool>`: conecta con timeout de 300 ms; envía `attach`; si `ok`, `copy_bidirectional(stdin+stdout, socket)` hasta EOF y devuelve `Ok(true)`; si no conecta o responde error, `Ok(false)`.

`cli.rs`: nuevo subcomando `broker` → `daemon::run(&home)`. En `serve`: `if config::is_shared(spec, name) && client::attach(&socket_path(), name).await? { return Ok(0) }` antes del camino actual.

`config.rs`:

```rust
pub const DEDICATED: &[&str] = &["chrome-bg","claude-in-chrome","playwright","x-playwright","lightpanda","obscura","screenwright","teams","claude-codex"];
pub fn is_shared(spec: &serde_json::Value, name: &str) -> bool {
    match spec.get("shared").and_then(|v| v.as_bool()) { Some(v) => v, None => !DEDICATED.contains(&name) }
}
pub fn socket_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(format!("/tmp/comandos-{}", nix::unistd::getuid())));
    base.join("comandos").join("broker.sock")
}
```

- [ ] **Step 4: Tests, clippy, commit**

Run: `nice -n 10 cargo test -p comandos-extensions -j 6 && cargo clippy -p comandos-extensions -- -D warnings`
Commit: `git add crates/comandos-extensions systemd/comandos-broker.service && git commit -m "feat(ext): broker compartido de servidores MCP por socket Unix y cliente fino en serve"`.

---

### Task 10: `comandos hook claude` con paridad contra `cc-notify.sh`

**Files:**
- Create: `crates/comandos-runtime/src/hooks/mod.rs`, `crates/comandos-runtime/src/hooks/claude.rs`, `crates/comandos-runtime/src/hooks/state_file.rs`, `crates/comandos-runtime/src/hooks/events_jsonl.rs`, `crates/comandos-runtime/src/hooks/notify_http.rs`, `crates/comandos-runtime/tests/hook_claude_parity.rs`, `crates/comandos-runtime/tests/fixtures/hooks/*.json` (payloads reales anonimizados de `UserPromptSubmit`, `Stop`, `Notification` (`permission_prompt`, `idle_prompt`), `SessionEnd`), `crates/comandos-runtime/tests/support/fake_notifyd.rs` (servidor HTTP falso en Rust con `hyper` que guarda cada cuerpo POST en un `Vec<String>` compartido)
- Modify: `crates/comandos-cli/src/main.rs` (rama `Command::Hook` → `comandos_runtime::hooks::run(&args)`), `crates/comandos-runtime/src/lib.rs` (`pub mod hooks;`)

**Interfaces:**
- Consumes: `comandos_store::intake` y `comandos_runtime::events_cli::run` (intake N1, ya migrado), variables de entorno de Claude Code (`CLAUDE_PROJECT_DIR`, `TMUX_PANE`), config `~/.claude/hooks/cc-notify.conf` (`VOLUME`, `CC_LANG`, `TELEGRAM_ENABLED`, …: leer con el mismo parser `KEY=valor` que bash `source`, solo claves conocidas).
- Produces: `hooks::run(args: &[String]) -> i32` que, para `claude`, lee el JSON de stdin y produce **los mismos efectos observables** que `hooks/cc-notify.sh`: (a) `~/.claude/hooks/state/<clave>.json` con las mismas claves y valores (clave de proyecto `<proj_file>--<sesión>--<pane>` como el bash); (b) una línea en `~/.claude/hooks/events.jsonl` con el mismo JSON; (c) intake N1 en SQLite (misma fila que hoy escribe `lib/event_intake.py` — ya cubierto por `events_cli`); (d) POST `http://127.0.0.1:4778/notify` con el mismo cuerpo cuando el bash lo haría; (e) `pw-play` con el mismo archivo y volumen (en pruebas, `PATH` apunta a un `pw-play` falso que registra argumentos). Las llamadas a Telegram y voz se cubren igual con binarios/endpoints falsos capturados.

El oráculo es el propio script: el test ejecuta `hooks/cc-notify.sh` y `comandos hook claude` con el mismo payload, mismo `HOME` temporal (copias independientes), mismo `PATH` falso y mismo `fake_notifyd.py`, y compara: el estado JSON (como `Value`), la última línea de `events.jsonl` (como `Value`, ignorando únicamente `ts` si difiere en ≤2 s), el cuerpo del POST capturado, y la línea registrada por `pw-play` falso.

- [ ] **Step 1: Capturar fixtures**

Copiar cinco payloads reales de `~/.claude/hooks/events.jsonl` (campos `session_id`, `transcript_path` y rutas reemplazados por valores de prueba) a `crates/comandos-runtime/tests/fixtures/hooks/{prompt,stop,notification_permission,notification_idle,session_end}.json`. `tests/support/fake_notifyd.rs`: `pub fn spawn(port: u16) -> (JoinHandle<()>, Arc<Mutex<Vec<String>>>)` con `hyper`; el test lo arranca en el puerto 4778 solo si está libre y, si no, pasa `COMANDOS_NOTIFYD_URL` a ambos procesos (añadir esa variable opcional al bash y al Rust para pruebas; por defecto `http://127.0.0.1:4778`).

- [ ] **Step 2: Test de paridad que falla**

`crates/comandos-runtime/tests/hook_claude_parity.rs`:

```rust
use std::{fs, path::{Path, PathBuf}, process::{Command, Stdio}, io::Write, thread, time::Duration};
use serde_json::Value;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

fn fresh_home(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("comandos-hook-{tag}"));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".claude/hooks/state")).unwrap();
    fs::write(home.join(".claude/hooks/cc-notify.conf"), "VOLUME=12\nCC_LANG=es\nTELEGRAM_ENABLED=0\nVOICE_ENABLED=0\n").unwrap();
    home
}

fn fake_bin(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for name in ["pw-play", "paplay", "notify-send", "curl"] {
        let p = dir.join(name);
        fs::write(&p, format!("#!/bin/sh\necho \"{name} $*\" >> \"$FAKE_LOG\"\n")).unwrap();
        use std::os::unix::fs::PermissionsExt; fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn run(cmd: &mut Command, home: &Path, payload: &str, fake: &Path) -> (Value, Value, String) {
    let log = home.join("fake.log");
    let mut child = cmd.env("HOME", home).env("FAKE_LOG", &log).env("PATH", format!("{}:/usr/bin:/bin", fake.display()))
        .env("CLAUDE_PROJECT_DIR", "/tmp/proyecto-prueba").env_remove("TMUX_PANE")
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    let state = fs::read_dir(home.join(".claude/hooks/state")).unwrap().next().map(|e| serde_json::from_str(&fs::read_to_string(e.unwrap().path()).unwrap()).unwrap()).unwrap_or(Value::Null);
    let events = fs::read_to_string(home.join(".claude/hooks/events.jsonl")).unwrap_or_default();
    let last: Value = events.lines().last().map(|l| serde_json::from_str(l).unwrap()).unwrap_or(Value::Null);
    (state, last, fs::read_to_string(log).unwrap_or_default())
}

fn strip_ts(mut v: Value) -> Value { if let Some(o) = v.as_object_mut() { o.remove("ts"); o.remove("time"); } v }

#[test]
fn claude_hook_matches_bash_for_all_fixtures() {
    let fake = std::env::temp_dir().join("comandos-hook-fakebin"); fake_bin(&fake);
    for name in ["prompt", "stop", "notification_permission", "notification_idle", "session_end"] {
        let payload = fs::read_to_string(root().join(format!("crates/comandos-runtime/tests/fixtures/hooks/{name}.json"))).unwrap();
        let h_bash = fresh_home(&format!("{name}-bash")); let h_rust = fresh_home(&format!("{name}-rust"));
        let bash = run(Command::new("bash").arg(root().join("hooks/cc-notify.sh")), &h_bash, &payload, &fake);
        let rust = run(Command::new(env!("CARGO_BIN_EXE_comandos")).args(["hook", "claude"]), &h_rust, &payload, &fake);
        assert_eq!(strip_ts(bash.0.clone()), strip_ts(rust.0.clone()), "estado difiere en {name}");
        assert_eq!(strip_ts(bash.1.clone()), strip_ts(rust.1.clone()), "events.jsonl difiere en {name}");
        assert_eq!(bash.2.replace(&h_bash.to_string_lossy().to_string(), "HOME"), rust.2.replace(&h_rust.to_string_lossy().to_string(), "HOME"), "efectos externos difieren en {name}");
    }
}

#[test]
fn truncated_payload_writes_nothing() {
    let fake = std::env::temp_dir().join("comandos-hook-fakebin"); fake_bin(&fake);
    let home = fresh_home("truncated");
    let (state, last, log) = run(Command::new(env!("CARGO_BIN_EXE_comandos")).args(["hook", "claude"]), &home, r#"{"hook_event_name":"Stop","session_id":"abc","transcript_pa"#, &fake);
    assert_eq!(state, Value::Null); assert_eq!(last, Value::Null); assert!(log.is_empty());
}
```
(El binario `comandos` vive en `comandos-cli`; para usarlo desde los tests de `comandos-runtime`, añadir `comandos-cli = { path = "../comandos-cli" }` en `[dev-dependencies]` del runtime; Cargo expone entonces `CARGO_BIN_EXE_comandos`.)

- [ ] **Step 3: Transcribir `hooks/cc-notify.sh` a Rust**

Leer `hooks/cc-notify.sh` completo. Estructura a reproducir en `claude.rs`:
1. `conf.rs`: cargar `cc-notify.conf` (defaults idénticos a las líneas 23-58 del bash).
2. Entrada: JSON de stdin (`hook_event_name`, `session_id`, `cwd`, `transcript_path`, `message`, `notification_type`, `stop_hook_active`…) o modo adaptador `--agent X --event working|waiting|done|end --cwd …` (líneas 71-150).
3. Identidad N1 y clave de estado (líneas 78-200): proyecto desde `cwd`, sesión y `pane_pid` desde `tmux display-message -p -t $TMUX_PANE` solo si `TMUX_PANE` es `%N`; el bash usa `tmux` del `PATH`: en Rust igual, con `std::process::Command`.
4. `turn_text()` (153): último mensaje del transcript JSONL para el resumen; `mdclean` (429) como funciones puras con tests unitarios propios (`#[cfg(test)]`) que copian ejemplos del bash.
5. `write_state`, `events_append` (263, 242-262: con el mismo lock `flock` sobre `events.jsonl.lock` — usar `nix::fcntl::flock`), `usage_lifecycle`/`usage_capture` (230-428) → llamar a `comandos_store::usage::{capture_hook, lifecycle}` de la Tarea 10b (Rust; el bash lanza `cc_usage.py lifecycle` en segundo plano, el Rust lo hace en una tarea `tokio::spawn_blocking` con el mismo efecto en SQLite).
6. `notify_desktop` (438) → POST a `127.0.0.1:4778/notify` con `hyper` (el bash usa `curl -s -m 2`); `notify_voice` (457); Telegram.
7. Intake N1: llamar a `events_cli::run` con los mismos argumentos que el bash pasa a `lib/event_intake.py` (línea 205 en adelante).

Al terminar cada bloque, correr el test de paridad y ajustar hasta igualar.

- [ ] **Step 4: Tests, clippy, commit**

Run: `nice -n 10 cargo test -p comandos-runtime --test hook_claude_parity -j 6 && cargo clippy -p comandos-runtime -- -D warnings`
Commit: `git add crates && git commit -m "feat(hooks): comandos hook claude con paridad verificada contra cc-notify.sh"`.

---

### Task 10b: Contabilidad de uso del hook en Rust (`capture-hook`, `lifecycle`, `tool-event`)

**Files:**
- Create: `crates/comandos-store/src/usage.rs`, `crates/comandos-store/tests/usage_parity.rs`
- Modify: `crates/comandos-store/src/lib.rs` (`pub mod usage;`)
- Oráculo: `python3 bin/cc_usage.py capture-hook|lifecycle|tool-event` (líneas 3037-3050 de `bin/cc_usage.py` muestran los tres puntos de entrada; cada uno lee un JSON por stdin y escribe en la base de uso que abre `sqlite3.connect(db_path, timeout=10)`).

**Interfaces:**
- Produces:

```rust
pub fn capture_hook(conn: &rusqlite::Connection, payload: &serde_json::Value) -> Result<()>;
pub fn lifecycle(conn: &rusqlite::Connection, payload: &serde_json::Value) -> Result<()>;
pub fn tool_event(conn: &rusqlite::Connection, payload: &serde_json::Value) -> Result<()>;
pub fn open_usage_db(home: &std::path::Path) -> Result<rusqlite::Connection>;   // misma ruta y PRAGMAs que `cc_usage.py` (localizar `db_path` en el módulo Python)
```
Cada función reproduce las sentencias SQL que ejecuta la rama Python correspondiente (leerlas en `bin/cc_usage.py`: buscar las funciones a las que llaman las tres ramas de `main`), incluidas la creación de tablas si no existen y las claves de deduplicación.

- [ ] **Step 1: Test de paridad que falla**

`crates/comandos-store/tests/usage_parity.rs`: para cada subcomando, con dos `HOME` temporales idénticos, ejecutar `python3 bin/cc_usage.py <sub>` con el payload de fixture por stdin y la función Rust con el mismo payload; después volcar ambas bases con `sqlite3 <db> .dump` (vía `Command`) y comparar línea a línea ignorando solo las columnas de marca de tiempo de inserción si existen. Payloads: el `lifecycle` exacto que construye el bash (`{status,harness,tmux_session,tmux_pane,prompt_id,agent_session_id,source,confidence:"exact",at_ms}`), un `capture-hook` con los seis números `COMANDOS_USAGE_*` y un `tool-event` tomado de `cc-usage-tool.sh`.

- [ ] **Step 2: Implementar hasta igualar los volcados; `cargo test -p comandos-store --test usage_parity`; clippy; commit**

`git commit -m "feat(store): contabilidad de uso del hook (capture-hook, lifecycle, tool-event) en Rust con paridad SQL"`.

---

### Task 11: Adaptadores de Codex, Gemini, agy, Grok y opencode, y `cc-usage-tool.sh` / `cc-status.sh`

**Files:**
- Create: `crates/comandos-runtime/src/hooks/codex.rs`, `gemini.rs`, `agy.rs`, `grok.rs`, `opencode.rs`, `claude_usage.rs`, `claude_status.rs`; tests `crates/comandos-runtime/tests/hook_adapters_parity.rs`; `adapters/opencode-comandos.js` reducido al shim de 3 líneas que ejecuta `comandos hook opencode` con el JSON del evento.
- Oráculos: `adapters/codex-notify.sh`, `adapters/codex-hooks.sh`, `adapters/gemini-hooks.sh`, `adapters/agy-hooks.sh`, `adapters/grok-hooks.py`, `hooks/cc-usage-tool.sh`, `hooks/cc-status.sh`.

**Interfaces:**
- Consumes: `hooks::run` (Tarea 10) con el primer argumento `codex|codex-hooks|gemini|agy|grok|opencode|claude-usage|claude-status`.
- Produces: mismos efectos que cada script con el mismo payload (misma técnica de la Tarea 10: `HOME` temporal, binarios falsos, comparación de estado/eventos/POST).

- [ ] **Step 1**: Un fixture real por adaptador (payload de `agent-turn-complete` de Codex; evento de Gemini `AfterAgent`; `Stop` de agy en formato plano camelCase; un evento Grok; un `session.idle` de opencode; un `PostToolUse` con `usage` para `cc-usage-tool.sh`; un JSON de statusline para `cc-status.sh`).
- [ ] **Step 2**: Test de paridad por adaptador con la misma plantilla de la Tarea 10 (`run()` reutilizado desde un módulo común `tests/common/mod.rs`).
- [ ] **Step 3**: Implementar cada `*.rs` transcribiendo el script correspondiente (la lógica de `codex_state_file` del `codex-notify.sh` está en las líneas 1-25 mostradas arriba: clave `<proj>--<sesión>--<pane>` y comprobación `.agent == "codex"`).
- [ ] **Step 4**: `cargo test -p comandos-runtime -j 6` en verde; commit `feat(hooks): adaptadores de codex, gemini, agy, grok, opencode, usage y status en Rust`.

---

### Task 12 (controlador): Cutover de hooks y broker

Prerrequisitos: Tareas 9-11 en verde; `comandos install --stage` con el release nuevo.

- [ ] **Step 1**: Sombra: durante 30 minutos, añadir en una copia de `~/.claude/settings.json` de un **proyecto de prueba** (no el global) el hook Rust junto al bash y comparar `state/*.json` y `events.jsonl` producidos; no tocar el `settings.json` global todavía.
- [ ] **Step 2**: `comandos install --link cc-notify.sh && comandos install --link cc-usage-tool.sh && comandos install --link cc-status.sh`. Los agentes nuevos y los turnos nuevos de los agentes vivos ya usan Rust (los hooks se lanzan por turno; no hay proceso residente que interrumpir). Vigilar 10 minutos `~/.claude/hooks/events.jsonl` y el tablero.
- [ ] **Step 3**: `ln -sf ~/.local/share/comandos/bin/comandos ~/.local/bin/codex-notify.sh` no aplica: `~/.codex/config.toml` apunta a `adapters/codex-notify.sh` del repo principal. Sustituir ese archivo del repo principal por un envoltorio de una línea `exec "$HOME/.local/share/comandos/bin/comandos" hook codex "$@"` **solo después** de que el test de paridad de Codex esté en verde; igual para `gemini-hooks.sh`, `agy-hooks.sh`, `grok-hooks.py` y el plugin de opencode.
- [ ] **Step 4**: Broker: `ln -sf <worktree>/systemd/comandos-broker.service ~/.config/systemd/user/ && systemctl --user daemon-reload && systemctl --user enable --now comandos-broker.service`. Desde ese momento `comandos ext serve` de sesiones nuevas se cuelga del broker. Medir con `cargo xtask rss` y con `ps` el número de procesos MCP tras abrir dos sesiones de prueba. Anotar en `docs/verification/cutover-broker.md`.
- [ ] **Step 5**: Reversión documentada: `systemctl --user disable --now comandos-broker.service` (los `serve` vuelven al modo directo automáticamente) y `comandos install --rollback <nombre>` por cada hook.

---

## Self-review

- **Cobertura de la spec (fases 0 y 1):** §3.3 descartes → Tarea 1; §4.1 binario único y symlinks → Tareas 2 y 6; §4.3 hooks y adaptadores → Tareas 10-11 (incluye el shim de opencode de §3.2); §4.7 proxy y broker con política `shared` → Tareas 4, 5, 8, 9; §5 cutover reversible y sombra → Tareas 7 y 12; §6 medición → Tarea 3. Lo que la spec asigna a fases posteriores (servidor, terminal, UI, escritorio, estado único) no está en este plan por diseño; Los tres puntos de entrada de `cc_usage.py` que usa el hook se migran en la Tarea 10b; el resto de `cc_usage.py` (analítica y rutas HTTP) pertenece a la fase 2. Ningún código nuevo en Python: los `.py`/`.sh` que aparecen son oráculos que corren dentro de los tests y se eliminan en la fase 6.
- **Placeholders:** la Tarea 10 pide transcribir el bash y lista sus bloques con números de línea; el comportamiento exacto lo fija el oráculo (test de paridad), no una descripción. La Tarea 11 sigue la misma plantilla con fixtures reales.
- **Consistencia de tipos:** `resolve`/`Command` (Tarea 2) se usa igual en la Tarea 6; `Mux`/`Outbound` (Tarea 8) en la Tarea 9; `hooks::run(&[String]) -> i32` en Tareas 10-11; `comandos_extensions::cli::run(Vec<String>) -> Result<i32>` en Tareas 2 y 9.
- **Review Focus:** 1 → `late_response_after_client_gone` (T8); 2 → `concurrent_initialize_single_upstream` (T8); 3 → `truncated_payload_writes_nothing` (T10); 4 → `dedicated_server_never_shared` (T9); 5 → `dispatch_by_argv0_and_explicit` (T2).
