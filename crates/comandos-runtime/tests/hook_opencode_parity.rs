//! Paridad de `comandos hook opencode` (con el shim nuevo `adapters/opencode-comandos.js`)
//! contra el plugin original (copia literal en `tests/fixtures/hooks/oracle/`). Los dos
//! corren en el MISMO proceso de node (mismo pid y tick de arranque para el registro
//! en `native-processes`), con el SDK de sesiones falso del fixture. El POST del
//! original a `/event` de cc-dash se sustituye por lo que cc-dash hacía con él
//! (validar y lanzar `cc-notify.sh --agent opencode --event E --cwd DIR`), con el
//! bash real detrás; el Rust entrega en proceso. Se comparan todos los efectos.
#[path = "support/fake_notifyd.rs"]
mod fake_notifyd;
#[path = "support/parity.rs"]
mod parity;

use parity::{
    Window, collect, fake_bin, install_oracle_notify, install_rust_notify_stub, lines,
    native_records, wait_for_delivery,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CONF: &str = "VOLUME=12\nCC_LANG=es\nTELEGRAM_ENABLED=0\nSPEAK_ATTENTION=0\nSPEAK_DONE=0\n";

/// Arnés de node: carga el original con `HOME=A` y el shim con `HOME=B`.
const HARNESS: &str = r#"
import { readFileSync, readdirSync, appendFileSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
const [orig, shim, homeA, homeB, fixture] = process.argv.slice(2);
const fx = JSON.parse(readFileSync(fixture, 'utf8'));
const client = { session: { get: async ({ path: { id } }) => ({ data: fx.sessions[id] ?? null }) } };
const side = (home) => {
  process.env.HOME = home;
  process.env.FAKE_LOG = join(home, 'fake.log');
  process.env.FAKE_POSTS = join(home, 'posts.log');
  process.env.TMPDIR = join(home, 'tmp');
};
// Lo que hacía `POST /event` de cc-dash (bin/cc-dash) con el cuerpo del plugin.
globalThis.fetch = async (url, init) => {
  appendFileSync(join(process.env.HOME, 'dash.log'), `${init.method} ${url} ${init.body}\n`);
  const data = JSON.parse(init.body);
  const agent = String(data.agent ?? '').slice(0, 16);
  const ev = String(data.event ?? '');
  const cwd = [...String(data.cwd ?? '')].slice(0, 300).join('');
  if (!/^[a-z][a-z0-9_-]{1,15}$/.test(agent) || !['working', 'waiting', 'done', 'end'].includes(ev)
      || !cwd.startsWith('/')) return { ok: false };
  spawnSync(join(process.env.HOME, '.claude/hooks/cc-notify.sh'),
    ['--agent', agent, '--event', ev, '--cwd', cwd], { stdio: 'ignore', env: process.env });
  return { ok: true };
};
// `__harness_delete_record`: alguien borra el registro del disco; el plugin sigue
// fusionando contra el que guarda en memoria.
// El oráculo (`trap wait`) termina sus trabajos de fondo antes del siguiente
// evento; del lado Rust se espera igual al proceso de entrega (`hook __deliver`)
// de ese `HOME`, o el orden working → waiting → done de la base de uso quedaría
// al azar (tampoco lo garantiza el `python3 cc_usage.py … &` del bash).
const delivering = (home) => readdirSync('/proc').some((pid) => {
  try {
    return readFileSync(`/proc/${pid}/cmdline`, 'latin1').includes('__deliver')
      && readFileSync(`/proc/${pid}/environ`, 'latin1').split('\0').includes(`HOME=${home}`);
  } catch { return false; }
});
const settle = async (home) => {
  const deadline = Date.now() + 15000;
  while (delivering(home) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 10));
};
const play = async (plugin, home) => {
  for (const event of fx.events) {
    if (event.type === '__harness_delete_record') {
      rmSync(join(home, '.claude/hooks/native-processes', `${process.pid}.json`), { force: true });
    } else {
      await plugin.event?.({ event });
      await settle(home);
    }
  }
};
side(homeA);
await play(await (await import(pathToFileURL(orig).href)).Comandos({ directory: fx.directory, client }), homeA);
side(homeB);
await play(await (await import(pathToFileURL(shim).href)).Comandos({ directory: fx.directory, client }), homeB);
console.log(process.pid);
"#;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks")
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
fn which(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("la prueba necesita `{name}` en el PATH"))
}

fn prepare(dir: &Path, side: &str) -> PathBuf {
    let home = dir.join(side);
    fs::create_dir_all(home.join(".claude/hooks/state")).unwrap();
    fs::create_dir_all(home.join("tmp")).unwrap();
    fs::write(home.join(".claude/hooks/cc-notify.conf"), CONF).unwrap();
    if side == "bash" {
        install_oracle_notify(&home, &root());
    } else {
        install_rust_notify_stub(&home);
    }
    home
}

fn run(dir: &Path, silent: bool, tmux: bool, url: &str) -> (PathBuf, PathBuf, Option<u32>) {
    let _ = fs::remove_dir_all(dir);
    let fake = dir.join("fakebin");
    fake_bin(&fake);
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos-hook"), fake.join("comandos")).unwrap();
    let (h_bash, h_rust) = (prepare(dir, "bash"), prepare(dir, "rust"));
    let harness = dir.join("harness.mjs");
    fs::write(&harness, HARNESS).unwrap();
    let node = which("node");
    let mut command = Command::new(&node);
    command
        .arg(&harness)
        .arg(fixtures().join("oracle/opencode-comandos.js"))
        .arg(root().join("adapters/opencode-comandos.js"))
        .arg(&h_bash)
        .arg(&h_rust)
        .arg(fixtures().join("adapters/opencode_session.json"))
        .env_clear()
        .current_dir(dir)
        .env(
            "PATH",
            format!(
                "{}:{}:/usr/bin:/bin",
                fake.display(),
                node.parent().unwrap().display()
            ),
        )
        .env("LANG", "C.UTF-8")
        .env("FAKE_PANE_PID", std::process::id().to_string())
        // Nunca el cc-notifyd real (4778): el Rust apunta al falso.
        .env("COMANDOS_NOTIFYD_URL", url);
    if silent {
        command.env("COMANDOS_SILENT_AGENT", "1");
    }
    // Bajo tmux la clave del estado es la del pane en los dos lados (el arnés lanza
    // `cc-notify.sh` con el entorno de OpenCode, como el Rust).
    if tmux {
        command.env("TMUX_PANE", "%7");
    }
    let out = command.stderr(Stdio::inherit()).output().unwrap();
    assert!(out.status.success(), "el arnés de node falló");
    let pid = String::from_utf8_lossy(&out.stdout).trim().parse().ok();
    (h_bash, h_rust, pid)
}

/// Bajo tmux la interacción se completa (working → done): su `duration_ms` depende
/// de cuánto tardó cada lado; los dos instantes ya se normalizan.
fn without_duration(row: String) -> String {
    match row.strip_prefix("  usage_interactions: ") {
        Some(cells) => {
            let mut cells: Vec<&str> = cells.split(" | ").collect();
            if cells.len() > 10 && cells[10].parse::<i64>().is_ok() {
                cells[10] = "D";
            }
            format!("  usage_interactions: {}", cells.join(" | "))
        }
        None => row,
    }
}

#[test]
fn opencode_shim_matches_original_plugin() {
    let (port, _server, bodies) = fake_notifyd::spawn(0);
    for (silent, tmux) in [(false, false), (true, false), (false, true)] {
        let dir = std::env::temp_dir().join(format!(
            "comandos-opencode-{}-{silent}-{tmux}",
            std::process::id()
        ));
        let start_ms = now_ms();
        bodies.lock().unwrap().clear();
        let (h_bash, h_rust, pid) = run(&dir, silent, tmux, &format!("http://127.0.0.1:{port}"));
        wait_for_delivery(&h_rust);
        let window = Window {
            start_ms,
            end_ms: now_ms(),
        };
        let bash_posts = lines(&h_bash.join("posts.log"));
        let rust_posts = std::mem::take(&mut *bodies.lock().unwrap());
        let dash = lines(&h_bash.join("dash.log"));
        if silent {
            assert!(dash.is_empty(), "{dash:?}");
        } else {
            // working, waiting, done (subagente), done, waiting y working (sesión desconocida).
            assert_eq!(dash.len(), 6, "{dash:?}");
            for line in &dash {
                assert!(
                    line.starts_with("POST http://127.0.0.1:4777/event {\"agent\":\"opencode\""),
                    "{line}"
                );
            }
        }
        if tmux {
            assert!(
                h_rust
                    .join(".claude/hooks/state/proyecto-opencode--fake-sess--7.json")
                    .exists(),
                "clave del pane"
            );
        }
        let mut bash = collect(&h_bash, Some(0), bash_posts, window);
        let mut rust = collect(&h_rust, Some(0), rust_posts, window);
        for effects in [&mut bash, &mut rust] {
            effects.usage = effects.usage.drain(..).map(without_duration).collect();
        }
        assert_eq!(bash.state, rust.state, "estado");
        assert_eq!(bash.events, rust.events, "events.jsonl");
        assert_eq!(bash.posts, rust.posts, "POST a cc-notifyd");
        assert_eq!(bash.log, rust.log, "binarios falsos");
        assert_eq!(bash.intake, rust.intake, "evento N1");
        assert_eq!(bash.usage, rust.usage, "base de uso");
        assert_eq!(bash, rust);
        let (natives_bash, natives_rust) = (
            native_records(&h_bash, None, window),
            native_records(&h_rust, None, window),
        );
        assert_eq!(natives_bash, natives_rust, "native-processes");
        assert_eq!(
            natives_rust.len(),
            if silent { 0 } else { 2 },
            "{natives_rust:?}"
        );
        if let (false, Some(pid)) = (silent, pid) {
            assert_eq!(natives_rust[0].0, format!("{pid}.json"));
        }
        let _ = fs::remove_dir_all(&dir);
    }
}

/// El shim usa `~/.local/share/comandos/bin/comandos` si existe y, si no, el del
/// `PATH`.
#[test]
fn shim_prefers_the_installed_comandos() {
    let dir = std::env::temp_dir().join(format!("comandos-opencode-bin-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let (home, path_bin, log) = (dir.join("home"), dir.join("path"), dir.join("log"));
    let local = home.join(".local/share/comandos/bin");
    fs::create_dir_all(&local).unwrap();
    fs::create_dir_all(&path_bin).unwrap();
    for (place, tag) in [(&local, "local"), (&path_bin, "path")] {
        let script = place.join("comandos");
        fs::write(
            &script,
            format!("#!/bin/sh\ncat >/dev/null\nprintf '{tag} %s\\n' \"$*\" >> \"$LOG\"\n"),
        )
        .unwrap();
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    let harness = dir.join("harness.mjs");
    fs::write(
        &harness,
        r#"import { pathToFileURL } from 'node:url';
const client = { session: { get: async () => ({ data: null }) } };
const p = await (await import(pathToFileURL(process.argv[2]).href)).Comandos({ directory: '/x', client });
await p.event({ event: { type: 'session.idle', properties: { sessionID: 's' } } });
"#,
    )
    .unwrap();
    let node = which("node");
    let play = || {
        let out = Command::new(&node)
            .arg(&harness)
            .arg(root().join("adapters/opencode-comandos.js"))
            .env_clear()
            .env("HOME", &home)
            .env("LOG", &log)
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    path_bin.display(),
                    node.parent().unwrap().display()
                ),
            )
            .output()
            .unwrap();
        assert!(out.status.success());
    };
    play();
    fs::remove_file(local.join("comandos")).unwrap();
    play();
    assert_eq!(
        lines(&log),
        vec!["local hook opencode", "path hook opencode"]
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Un `comandos` colgado no detiene a OpenCode: el shim lo corta a los 5 s.
#[test]
fn shim_gives_up_after_five_seconds() {
    let dir = std::env::temp_dir().join(format!("comandos-opencode-tmo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let local = dir.join("home/.local/share/comandos/bin");
    fs::create_dir_all(&local).unwrap();
    let script = local.join("comandos");
    fs::write(&script, "#!/bin/sh\nexec sleep 60\n").unwrap();
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let harness = dir.join("harness.mjs");
    fs::write(
        &harness,
        r#"import { pathToFileURL } from 'node:url';
const client = { session: { get: async () => ({ data: null }) } };
const p = await (await import(pathToFileURL(process.argv[2]).href)).Comandos({ directory: '/x', client });
await p.event({ event: { type: 'session.idle', properties: { sessionID: 's' } } });
"#,
    )
    .unwrap();
    let node = which("node");
    let started = std::time::Instant::now();
    let out = Command::new(&node)
        .arg(&harness)
        .arg(root().join("adapters/opencode-comandos.js"))
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    assert!(out.status.success());
    assert!(
        (std::time::Duration::from_secs(4)..std::time::Duration::from_secs(15)).contains(&elapsed),
        "{elapsed:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}
