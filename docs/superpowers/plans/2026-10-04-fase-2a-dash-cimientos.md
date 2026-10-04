# Fase 2a — Releases versionadas, `comandos dash` como frente y arnés de paridad

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** poner el servidor Rust (`comandos dash`) delante del tablero en el puerto 4777 sin portar todavía ninguna ruta de dominio: puerta de seguridad y estáticos en Rust, el resto reenviado al `cc-dash` Python movido a 4781, con releases versionadas del binario (rollback Rust→Rust) y un arnés que compara Rust contra Python byte a byte. Todo lo que ve Jesús sigue igual; a partir de aquí cada dominio se corta y revierte por separado (Fase 2b).

**Architecture:** `comandos dash` usa `comandos-server` (hyper 1 + tokio, ya con admisión y límites) y añade un enrutador con tres clases de ruta: *estático* (archivos de `H/dash`, sin listados), *reenviada* (proxy HTTP/1.1 a `127.0.0.1:4781` con cabeceras y cuerpo intactos, respuesta en streaming para el long-poll de 25 s) y, en la Fase 2b, *nativa*. La puerta de seguridad es `comandos_core::dashboard_access` (ya portada con fixture de paridad); el Python vuelve a evaluarla con las mismas cabeceras y el mismo peer loopback, así que decide igual. El binario instalado pasa a ser un enlace `bin/comandos → releases/<id>/comandos` (id = sha256 del binario), con `install --rollback-release`. `xtask parity` levanta ambos servidores sobre copias idénticas de `~/.claude/hooks` y compara las respuestas de un fixture de peticiones.

**Tech Stack:** Rust 1.96 edición 2024; crates existentes `comandos-core`, `comandos-store`, `comandos-runtime`, `comandos-extensions`, `comandos-server`, `comandos-cli`, `xtask`; hyper 1.11 (`server`+`client`, `http1`), tokio 1.53, `http-body-util`; sin dependencias nuevas salvo activar la feature `client` de hyper en `comandos-server` y `hyper-util` `client-legacy` si hace falta (preferir cliente http1 manual con `hyper::client::conn::http1`, que ya está en `dev-dependencies`). Oráculo: `bin/cc-dash` (Python 3.10) solo en tests y arnés.

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` §4.2, §5, §6, §7 y sus enmiendas del 4-oct (sección «Enmiendas»); inventario de rutas en `docs/research/2026-10-04-fase-2-inventario.md` (161 rutas; §2 puerta; §3 servidor; §6 unidades; §9 riesgos).

## Global Constraints

- Reglas de oro (spec §2): no se omite ninguna función visible; lo más ligero posible; **no se interrumpe ninguna sesión** (nunca `cc-app`, `tmux.service`, ni el tmux del usuario); el tablero muestra exactamente lo mismo. Ninguna prueba toca `~/.claude/hooks`, el puerto 4777/4778/4781, `~/.local/share/comandos` ni systemd del usuario: siempre `HOME` temporal, puertos efímeros (`127.0.0.1:0`) y `tmux -L comandos-test`.
- Cero Python/bash/JS nuevos en el producto. `bin/cc-dash` y sus `lib/*.py` no se modifican en este plan (siguen siendo el oráculo y el servidor heredado). Una unidad systemd nueva sí vale.
- Paridad byte a byte con el Python en todo lo que atiende Rust, salvo las **diferencias aceptadas** de este plan (estáticos, abajo): se documentan en `docs/verification/cutover-dash.md` y el arnés las conoce.
- Comentarios en español, identificadores en inglés; `comandos-server` tiene comentarios en inglés heredados: el código nuevo va en español, el existente no se retraduce.
- `cargo build --workspace`, `cargo test --workspace --no-fail-fast`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` en verde en cada commit. `CARGO_TARGET_DIR=<worktree>/.build/target`, `nice -n 10`, `-j 6`. Sin `unsafe`. Dependencias nuevas: ninguna sin ruling.
- Cutovers, stage y reinicios de unidades los hace **el controlador**, nunca el implementador. El implementador deja el binario, la unidad y el procedimiento documentado.
- Commits pequeños por tema con `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`; nunca `git add -A`; `docs/superpowers` está en `.gitignore` → `git add -f`.

## Rulings del controlador que fijan este plan (registrar en el ledger si se discuten)

1. **Transición por proxy inverso, no por cutover único** (inventario §9.2): el frente Rust en 4777 reenvía a Python en 4781 todo lo que aún no es nativo. Cada dominio se corta en la Fase 2b con un interruptor por ruta y se revierte en segundos sin tocar la unidad. Coste si falla: dos procesos durante la transición (el Python conserva su memoria hasta el final).
2. **Estáticos desde disco, no `include_bytes!`** hasta la Fase 3: otras sesiones editan `dash/*.js` en vivo y `cc-dash` los sirve al instante; embeberlos rompería ese flujo. Diferencias aceptadas respecto a `SimpleHTTPRequestHandler`: sin listados de directorio (404), directorio sin barra 404 (no 301), 404 JSON `{"error":"No encontrado"}` en vez de HTML, tabla MIME fija (no `/etc/mime.types`), sin `Server`/`Date` de la librería. Se conservan `Cache-Control: no-store`, `Last-Modified`, 304 por `If-Modified-Since`, HEAD, `/` → `index.html` y la regla de *asset público* de la puerta.
3. **Las 39 rutas sin llamador vivo** (inventario §1.12) se portan en la Fase 2b como `410 {"error":"Ruta retirada"}` (el patrón de `/operator`), con la lista en `cutover-dash.md`; si el journal muestra un llamador real, se portan de verdad. En este plan siguen reenviadas al Python (sin cambio).
4. **Bucles de fondo** (modelos, noticias, push, límites, Pomodoro, `restore_requested_webterm`, `motor_queue_resume`) siguen en el Python heredado durante la 2a y 2b hasta que su dominio sea el último en moverse; nunca corren dos veces.
5. **El id de release es el sha256 del binario** (12 hex): no hay metadatos de build, binarios idénticos comparten release, reproducible.
6. **`--no-open`** se acepta y se ignora; `comandos dash` nunca abre el navegador (la unidad siempre lo pasa). El mensaje de arranque es el mismo que el Python: `Centro Claude corriendo en http://127.0.0.1:<puerto>  (Ctrl+C para salir)`.
7. **Tailscale no cambia**: `https://<host>/` → 4777 sigue siendo el tablero (ahora Rust); `/term` → 4780 y `:8443` → 4779 siguen con ttyd (se retira en la Fase 3 con `comandos-term`). `POST /remote-off` sigue reenviado al Python y se trata en la 2b (no debe ejecutar `tailscale serve reset`).
8. **`notifyd`** se porta en la Fase 4 con GTK; **SSE** `/events/stream` llega con la UI de la Fase 3. Ambos registrados en las enmiendas de la spec.

## Review Focus

1. Una petición remota (`X-Forwarded-For: 100.64.0.9`, Host `*.ts.net`, token correcto) a una ruta reenviada debe llegar al Python **con las mismas cabeceras** y recibir exactamente la respuesta del Python (status, `Content-Type`, `Content-Length`, cuerpo); la puerta Rust y la Python deben coincidir en las 4 clases (local directo, proxy dev, remoto con token, remoto sin token → 401 idéntico). Test en Task 4.
2. `GET /notices/watch` reenviado tarda hasta 25 s: ni `handler_timeout` ni `write_timeout` lo cortan y el cliente recibe el cuerpo íntegro al final. Test con un heredado falso que tarda 3 s con `handler_timeout` de 1 s. Task 4.
3. Con el Python caído, una ruta reenviada responde `502 {"error":"Servidor heredado no disponible"}` en < 1 s, y los estáticos siguen sirviéndose. Task 4.
4. `install --stage` sobre una instalación vieja (archivo regular `bin/comandos`) la convierte en release sin romper los symlinks `cc-*` que apuntan a `bin/comandos`; `--rollback-release` vuelve al binario anterior; un daemon ya en ejecución sigue vivo (inodo retenido). Task 1.
5. El arnés con copias de `H/` no toca el `HOME` real: los GET con efectos (`/state` escribe `app-tab-models.json`) solo escriben en las copias; el arnés falla si `HOME` del proceso hijo apunta a `/home/someguy`. Task 5.

---

### Task 1: Releases versionadas del binario

**Files:**
- Modify: `crates/comandos-cli/src/install.rs` (hoy `stage` copia a `bin/comandos`, `:90-105`)
- Create: `crates/comandos-cli/src/install/release.rs`
- Test: `crates/comandos-cli/tests/install_release.rs`
- Modify: `docs/verification/cutover-ext.md` (sección «Releases»)

**Interfaces:**
- Consumes: `install::run(args) -> Result<i32, String>`, `parse`, `Action` (`install.rs:22-56`), `move_file` (`:108-126`).
- Produces: `install::release::{stage_release(home, exe) -> Result<Release, String>, rollback_release(home) -> Result<Release, String>, list_releases(home) -> Result<Vec<Release>, String>}`; `pub struct Release { pub id: String, pub path: PathBuf, pub current: bool }`. Nuevas acciones CLI: `--stage` (ahora versionado), `--rollback-release`, `--releases`.

Diseño: `~/.local/share/comandos/releases/<id>/comandos` (id = 12 hex del sha256 del binario); `~/.local/share/comandos/bin/comandos` es un **symlink relativo** `../releases/<id>/comandos`; `releases/previous` es un archivo con el id anterior. Si `bin/comandos` es un archivo regular (instalación de la Fase 1), `--stage` lo mueve primero a `releases/<su sha>/comandos` y lo apunta como `previous`. El swap es `symlink(tmp) + rename(tmp → bin/comandos)`. Se conservan las 5 releases más recientes por mtime más `previous` y la actual; el resto se borra. `--rollback-release` intercambia actual y `previous`.

- [ ] **Step 1: Test de stage versionado y rollback**

```rust
// crates/comandos-cli/tests/install_release.rs
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn comandos() -> &'static str {
    env!("CARGO_BIN_EXE_comandos")
}
fn home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("cmd-rel-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&h);
    fs::create_dir_all(&h).unwrap();
    h
}
fn run(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(comandos())
        .arg("install")
        .arg("--home")
        .arg(home)
        .args(args)
        .output()
        .unwrap()
}
fn sha12(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(fs::read(path).unwrap());
    format!("{:x}", h.finalize())[..12].to_string()
}

#[test]
fn stage_installs_a_release_and_links_bin_comandos_to_it() {
    let h = home("stage");
    assert!(run(&h, &["--stage"]).status.success());
    let bin = h.join(".local/share/comandos/bin/comandos");
    let target = fs::read_link(&bin).unwrap();
    let id = sha12(Path::new(comandos()));
    assert_eq!(target, Path::new("../releases").join(&id).join("comandos"));
    let real = h.join(".local/share/comandos/releases").join(&id).join("comandos");
    assert!(real.metadata().unwrap().permissions().mode() & 0o111 != 0);
    // Idempotente: el mismo binario no crea otra release.
    assert!(run(&h, &["--stage"]).status.success());
    assert_eq!(fs::read_dir(h.join(".local/share/comandos/releases")).unwrap()
        .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_dir()).count(), 1);
}

#[test]
fn a_fase_1_regular_file_becomes_the_previous_release_and_rollback_restores_it() {
    let h = home("legacy");
    let bin = h.join(".local/share/comandos/bin/comandos");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::write(&bin, b"#!/bin/sh\necho viejo\n").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    let old_id = sha12(&bin);
    assert!(run(&h, &["--stage"]).status.success());
    assert!(bin.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(h.join(".local/share/comandos/releases/previous")).unwrap().trim(), old_id);
    let out = run(&h, &["--rollback-release"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fs::read_link(&bin).unwrap(), Path::new("../releases").join(&old_id).join("comandos"));
    assert_eq!(fs::read_to_string(h.join(".local/share/comandos/releases/previous")).unwrap().trim(),
        sha12(Path::new(comandos())));
}

#[test]
fn releases_lists_current_first_and_prunes_to_five() {
    let h = home("prune");
    let rel = h.join(".local/share/comandos/releases");
    for i in 0..7 {
        let d = rel.join(format!("{i:012x}"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("comandos"), [i as u8]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    assert!(run(&h, &["--stage"]).status.success());
    let dirs = fs::read_dir(&rel).unwrap()
        .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_dir()).count();
    assert_eq!(dirs, 5, "actual + 4 más recientes");
    let out = run(&h, &["--releases"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.lines().next().unwrap().starts_with(&format!("* {}", sha12(Path::new(comandos())))));
}

#[test]
fn rollback_without_previous_fails_cleanly() {
    let h = home("noprev");
    assert!(run(&h, &["--stage"]).status.success());
    let out = run(&h, &["--rollback-release"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no hay release anterior"));
}
```

`sha2` ya es dependencia del workspace (lo usa `comandos-extensions` para la clave del broker); añadirlo a `[dev-dependencies]` y `[dependencies]` de `comandos-cli` con la misma versión fijada del `Cargo.lock`.

- [ ] **Step 2: Correr y ver fallar** — `cargo test -p comandos-cli --test install_release` → falla: `--rollback-release`/`--releases` desconocidos (salida 2) y `bin/comandos` no es symlink.

- [ ] **Step 3: Implementar `install/release.rs`**

```rust
//! Releases versionadas del binario: `releases/<sha12>/comandos` y el enlace
//! `bin/comandos -> ../releases/<sha12>/comandos`. Un daemon en ejecución conserva su
//! inodo; `--rollback-release` intercambia el actual con `releases/previous`.
use std::{fs, io, path::{Path, PathBuf}};
use sha2::{Digest, Sha256};

pub struct Release { pub id: String, pub path: PathBuf, pub current: bool }

const KEEP: usize = 5;

pub fn stage_release(home: &Path, exe: &Path) -> Result<Release, String> {
    let share = home.join(".local/share/comandos");
    let releases = share.join("releases");
    let bin = share.join("bin/comandos");
    fs::create_dir_all(&releases).map_err(|e| format!("no se pudo crear {}: {e}", releases.display()))?;
    fs::create_dir_all(bin.parent().unwrap()).map_err(|e| e.to_string())?;
    // Una instalación de la Fase 1 (archivo regular) pasa a ser release "anterior".
    let previous = match bin.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => current_id(&bin),
        Ok(_) => {
            let id = sha12(&bin)?;
            let dir = releases.join(&id);
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            super::move_file(&bin, &dir.join("comandos"))?;
            Some(id)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("no se pudo leer {}: {e}", bin.display())),
    };
    let id = sha12(exe)?;
    let dir = releases.join(&id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join("comandos");
    if !target.exists() {
        let tmp = dir.join("comandos.tmp");
        fs::copy(exe, &tmp).map_err(|e| format!("no se pudo copiar a {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    }
    if previous.as_deref() != Some(id.as_str()) {
        if let Some(p) = &previous { fs::write(releases.join("previous"), format!("{p}\n")).map_err(|e| e.to_string())?; }
        swap_link(&bin, &id)?;
    }
    prune(&releases, &id, previous.as_deref())?;
    Ok(Release { id, path: target, current: true })
}

pub fn rollback_release(home: &Path) -> Result<Release, String> {
    let share = home.join(".local/share/comandos");
    let releases = share.join("releases");
    let bin = share.join("bin/comandos");
    let prev = fs::read_to_string(releases.join("previous")).ok().map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && releases.join(s).join("comandos").is_file())
        .ok_or("no hay release anterior a la que volver")?;
    let current = current_id(&bin).ok_or("bin/comandos no es un enlace a una release")?;
    swap_link(&bin, &prev)?;
    fs::write(releases.join("previous"), format!("{current}\n")).map_err(|e| e.to_string())?;
    Ok(Release { id: prev.clone(), path: releases.join(&prev).join("comandos"), current: true })
}

pub fn list_releases(home: &Path) -> Result<Vec<Release>, String> { /* ordenadas: actual primero, luego por mtime desc */ todo!() }

fn swap_link(bin: &Path, id: &str) -> Result<(), String> {
    let tmp = bin.with_extension("tmp-link");
    let _ = fs::remove_file(&tmp);
    std::os::unix::fs::symlink(Path::new("../releases").join(id).join("comandos"), &tmp).map_err(|e| e.to_string())?;
    fs::rename(&tmp, bin).map_err(|e| format!("no se pudo enlazar {}: {e}", bin.display()))
}
fn current_id(bin: &Path) -> Option<String> {
    let t = fs::read_link(bin).ok()?;
    t.parent()?.file_name()?.to_str().map(str::to_string)
}
fn sha12(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes))[..12].to_string())
}
fn prune(releases: &Path, current: &str, previous: Option<&str>) -> Result<(), String> { /* conserva KEEP más recientes + actual + previous */ todo!() }
```

El `todo!()` de `list_releases` y `prune` se escribe en este mismo paso (el fragmento marca la firma; la salida de `--releases` es una línea por release: `* <id>  <ruta>` para la actual, `  <id>  <ruta>` para las demás, de más a menos reciente). `install.rs`: `Action::Stage` llama a `stage_release(&home, &current_exe)`, nuevas `Action::RollbackRelease` y `Action::Releases`; `USAGE` documenta las tres. `link()` sigue apuntando los `cc-*` a `bin/comandos` (que ahora es symlink: `std::os::unix::fs::symlink` de un enlace a un enlace funciona; `fs::copy` no se usa aquí).

- [ ] **Step 4: Verde** — `cargo test -p comandos-cli`, clippy, fmt.
- [ ] **Step 5: Documentar** en `docs/verification/cutover-ext.md` («Releases»: diseño, comandos, qué pasa con daemons vivos) y commit: `feat(install): releases versionadas del binario con rollback Rust→Rust`.

---

### Task 2: `comandos dash` — esqueleto, puerta y arranque

**Files:**
- Create: `crates/comandos-server/src/dash/mod.rs` (config y arranque), `crates/comandos-server/src/dash/router.rs` (clasificación de rutas), `crates/comandos-server/src/dash/token.rs`
- Modify: `crates/comandos-server/src/lib.rs` (`pub mod dash;`; HEAD admitido para estáticos, ver abajo), `crates/comandos-server/Cargo.toml`
- Modify: `crates/comandos-cli/Cargo.toml` (dep `comandos-server`), `crates/comandos-cli/src/dispatch.rs` (alias `cc-dash` → `dash`, `Command::Dash`), `crates/comandos-cli/src/main.rs`
- Test: `crates/comandos-server/tests/dash_boot.rs`, `crates/comandos-cli/tests/dispatch.rs` (alias)

**Interfaces:**
- Consumes: `comandos_server::{serve, Config, Limits, Request, Reply, Handler}` (`lib.rs:32-105,348`), `comandos_core::dashboard_access::{security_gate, public_asset, request_admission}`.
- Produces: `comandos_server::dash::{DashConfig { port: u16, legacy_port: u16, home: PathBuf, dash_dir: PathBuf, token: Vec<u8> }, run(cfg, shutdown) -> io::Result<()>, parse_args(&[String]) -> Result<DashConfig, String>, load_token(home) -> io::Result<Vec<u8>>, dash_dir(home) -> Result<PathBuf, String>}`; `router::RouteClass::{Static, Forward}` (la 2b añade `Native`).

Comportamiento: argumentos `[puerto] [--no-open] [--legacy-port N]` (`--legacy-port` también por env `COMANDOS_DASH_LEGACY_PORT`, por defecto 4781). Token: `H/dash-token` recortado; si no existe o está vacío se crea con 0600 y 32 bytes aleatorios de `/dev/urandom` en base64 URL-safe sin `=` (43 caracteres, como `secrets.token_urlsafe(32)`). `dash_dir`: `COMANDOS_DASH_DIR` (debe ser directorio legible, si no `SystemExit` → salir 1 con el mismo texto `COMANDOS_DASH_DIR no es un directorio legible: <ruta>`), si no `H/dash`. Escucha `127.0.0.1:<puerto>` (IPv4, como el Python). SIGTERM/SIGINT → parada ordenada (`shutdown_grace` 2 s). Imprime en stdout `Centro Claude corriendo en http://127.0.0.1:<puerto>  (Ctrl+C para salir)`. `Limits`: `connections 256`, `header_bytes 64 KiB`, `buffered_wire_bytes 20_000_000 + 64 KiB`, `header_timeout 30 s`, `body_timeout 30 s`, `handler_timeout 120 s` (las clases de ruta de la 2b afinan por ruta), `write_timeout 30 s`.

HEAD: el transporte responde 501 a HEAD (`lib.rs:220-225`); el Python sirve HEAD de estáticos sin puerta. Cambiar la admisión para que HEAD se trate como GET sin cuerpo en la respuesta (el enrutador decide; para rutas no estáticas, 501 como hoy el transporte… **no**: el Python sirve HEAD de cualquier ruta por `SimpleHTTPRequestHandler.do_HEAD` → 404 HTML para las rutas API. Ruling: HEAD solo para estáticos; HEAD a ruta API → `404 {"error":"No encontrado"}`. Diferencia aceptada, documentar).

- [ ] **Step 1: Tests de arranque**

```rust
// crates/comandos-server/tests/dash_boot.rs
use comandos_server::dash::{dash_dir, load_token, parse_args};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

fn home(tag: &str) -> PathBuf {
    let h = std::env::temp_dir().join(format!("cmd-dash-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&h);
    fs::create_dir_all(h.join(".claude/hooks/dash")).unwrap();
    h
}

#[test]
fn args_default_to_4777_and_legacy_4781_and_ignore_no_open() {
    let cfg = parse_args(&["--no-open".into()], &home("args"), None).unwrap();
    assert_eq!((cfg.port, cfg.legacy_port), (4777, 4781));
    let cfg = parse_args(&["4790".into(), "--no-open".into(), "--legacy-port".into(), "4791".into()], &home("args2"), None).unwrap();
    assert_eq!((cfg.port, cfg.legacy_port), (4790, 4791));
    let cfg = parse_args(&[], &home("args3"), Some("4799")).unwrap();
    assert_eq!(cfg.legacy_port, 4799, "COMANDOS_DASH_LEGACY_PORT");
}

#[test]
fn token_is_created_once_with_0600_and_43_urlsafe_chars() {
    let h = home("token");
    let t = load_token(&h).unwrap();
    let path = h.join(".claude/hooks/dash-token");
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(t.len(), 43);
    assert!(t.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_'));
    assert_eq!(load_token(&h).unwrap(), t, "estable");
    fs::write(&path, "  fijo-123 \n").unwrap();
    assert_eq!(load_token(&h).unwrap(), b"fijo-123", "recortado como el Python");
}

#[test]
fn dash_dir_prefers_a_readable_override_and_rejects_a_bad_one() {
    let h = home("dir");
    assert_eq!(dash_dir(&h, None).unwrap(), h.join(".claude/hooks/dash"));
    let other = h.join("otro"); fs::create_dir_all(&other).unwrap();
    assert_eq!(dash_dir(&h, Some(other.to_str().unwrap())).unwrap(), other);
    let err = dash_dir(&h, Some("/definitivamente/no")).unwrap_err();
    assert!(err.starts_with("COMANDOS_DASH_DIR no es un directorio legible: "));
}
```

Más un test de proceso en `crates/comandos-cli/tests/dash_boot.rs`: lanzar `comandos dash 0`… el puerto 0 no sirve para conocer el puerto; usar `--port-file <ruta>` **no** (API nueva). Alternativa: elegir un puerto libre en el test (bind a 0, leer, cerrar, lanzar) y comprobar que stdout contiene `Centro Claude corriendo en http://127.0.0.1:<puerto>  (Ctrl+C para salir)` y que `GET /no-existe` desde loopback sin XFF responde `404 {"error":"No encontrado"}` con `Cache-Control: no-store`; después SIGTERM y salida 0 en < 3 s. El alias: `comandos-cli/tests/dispatch.rs` añade `("cc-dash", ["dash"])`.

- [ ] **Step 2: Ver fallar.**
- [ ] **Step 3: Implementar** `dash/mod.rs` (config, `run` que construye `Config` con `token`, `asset_exists` = `dash_dir.join(path)` es archivo tras `canonicalize` dentro de `dash_dir` **o sus symlinks** — `H/dash/*` son symlinks al repo, así que la comprobación es «el enlace resuelve a un archivo», no «está bajo dash_dir»), el `Handler` que llama a `router::classify(&request)` y despacha a `statics::serve` (Task 3) o `forward::relay` (Task 4) — en esta tarea ambos son `todo!()` sustituidos por `404 JSON`, para que el test pase y las tareas 3 y 4 los rellenen; `dash/token.rs`; `dash/router.rs` con `classify(method, path) -> RouteClass`: `Static` si el método es GET/HEAD y la ruta no empieza por ningún prefijo de `dashboard_access::API_GET` (o es asset público), `Forward` en caso contrario. `main.rs`/`dispatch.rs`: `Command::Dash(args)` → `comandos_server::dash::run` dentro de un runtime tokio `current_thread` (el servidor ya es monohilo de red: `comandos-server` usa `rt`, no `rt-multi-thread`).
- [ ] **Step 4: Verde, clippy, fmt, commit** `feat(dash): comandos dash arranca en 4777 con la puerta de seguridad portada y el token del Python`.

---

### Task 3: Estáticos desde `H/dash`

**Files:**
- Create: `crates/comandos-server/src/dash/statics.rs`
- Test: `crates/comandos-server/tests/dash_statics.rs`
- Modify: `crates/comandos-server/src/dash/router.rs` (llama a `statics`)

**Interfaces:**
- Produces: `statics::serve(dash_dir: &Path, request: &Request) -> Reply` (sin async; lectura de archivo con `tokio::task::spawn_blocking` desde el handler), `statics::mime_for(path: &str) -> &'static str`.

Reglas (paridad útil con `SimpleHTTPRequestHandler`, diferencias aceptadas en el ruling 2): ruta decodificada (`%20`), sin `..` ni componentes vacíos (rechazo 404), `/` → `index.html`; archivo existente → 200 con `Content-Type` de la tabla, `Content-Length`, `Last-Modified` en formato HTTP (`Sat, 04 Oct 2026 12:00:00 GMT`), `Cache-Control: no-store`; `If-Modified-Since` ≥ mtime (precisión de segundo, como Python) → 304 sin cuerpo; HEAD → cabeceras sin cuerpo; directorio o ausente → `404 {"error":"No encontrado"}`. Tabla MIME (los que hay en `dash/`, `assets/`, `vendor/`, `icons/`): `html text/html`, `js text/javascript`, `mjs text/javascript`, `css text/css`, `json application/json`, `webmanifest application/manifest+json`, `png image/png`, `svg image/svg+xml`, `ico image/vnd.microsoft.icon`, `woff2 font/woff2`, `woff font/woff`, `ttf font/ttf`, `wasm application/wasm`, `map application/json`, `txt text/plain`, `webp image/webp`, `jpg/jpeg image/jpeg`, `gif image/gif`, `mp3 audio/mpeg`, `ogg audio/ogg`, `wav audio/x-wav`; desconocido → `application/octet-stream`. Python añade `; charset=utf-8`? No: `SimpleHTTPRequestHandler.guess_type` devuelve el tipo pelado; conservar pelado.

- [ ] **Step 1: Tests** (`dash_statics.rs`): servidor real con `comandos_server::serve` y `dash::handler` sobre un `dash_dir` temporal con `index.html`, `app.js`, subdirectorio `assets/x.css` (como symlink a otro archivo, para cubrir `H/dash` → repo), un directorio vacío `vendor/`; casos: `GET /` devuelve el `index.html` con `text/html` y `Last-Modified`; `GET /app.js` `text/javascript`; `If-Modified-Since` igual al `Last-Modified` → 304 sin cuerpo; `HEAD /app.js` → cabeceras, cuerpo vacío; `GET /vendor/` → 404 JSON; `GET /..%2Fsecret` → 404; `GET /assets/x.css` por symlink → 200; `GET /nada.css` → 404; `GET /no.css` cuando existe pero es un asset público pedido con `X-Forwarded-For` remoto sin token → 200 (público) frente a `GET /state` igual → 401 (puerta). Reutilizar el cliente hyper de `tests/transport.rs` (copiar su helper `request()` a `tests/support/mod.rs` si no existe).
- [ ] **Step 2: Ver fallar. Step 3: Implementar. Step 4: Verde. Step 5: Commit** `feat(dash): estáticos de H/dash con Last-Modified/304, HEAD y tabla MIME fija`.

---

### Task 4: Reenvío al `cc-dash` heredado

**Files:**
- Create: `crates/comandos-server/src/dash/forward.rs`
- Test: `crates/comandos-server/tests/dash_forward.rs`, `crates/comandos-server/tests/dash_forward_python.rs` (oráculo real, `#[ignore]`-free pero salta con mensaje si no hay `python3`)
- Modify: `crates/comandos-server/Cargo.toml` (hyper `client` en `[dependencies]`)

**Interfaces:**
- Produces: `forward::relay(legacy: SocketAddr, request: Request) -> Result<Reply, HandlerError>`; `forward::is_hop_by_hop(name) -> bool`.

Comportamiento: conexión TCP nueva por petición a `127.0.0.1:<legacy_port>` (`hyper::client::conn::http1::handshake`, sin pool: el Python abre un hilo por conexión y cerrarla al terminar es lo más parecido a un navegador; el keep-alive se añade en la 2b si el bench lo pide); se reenvían método, target completo (ruta + query, tal cual llegó), todas las cabeceras salvo hop-by-hop (`connection`, `keep-alive`, `transfer-encoding`, `te`, `trailer`, `upgrade`, `proxy-*`) **en el orden recibido**, `Host` tal cual (hyper no debe sustituirlo: construir la petición con la cabecera `host` explícita), y el cuerpo ya leído (`Request.data` es el JSON parseado — **no sirve**: hay que reenviar los bytes originales. Añadir a `comandos_server::Request` el campo `pub body: Bytes` con el cuerpo crudo admitido; `data` sigue siendo el JSON parseado). La respuesta se devuelve como `Reply` con el status, las cabeceras del Python en orden (incluida `Connection: close` cuando venga) y el cuerpo en `ReplyBody::Stream` (frames según llegan; así el long-poll de 25 s no se corta: el `handler_timeout` solo cubre hasta recibir las cabeceras, y la lectura del cuerpo tiene su propio límite de inactividad de 60 s). Fallos: no se pudo conectar → `502 {"error":"Servidor heredado no disponible"}`; corte a media respuesta → se cierra el stream (el cliente ve la conexión cerrada, como con el Python). Nunca añadir `X-Forwarded-For`, `Via` ni `X-Real-IP`.

- [ ] **Step 1: Tests con heredado falso** (`dash_forward.rs`): un servidor hyper de prueba en el test que graba la petición recibida (método, target, cabeceras en orden, cuerpo) y responde según la ruta: `/eco` → 200 JSON con lo recibido; `/lento` → espera 3 s y responde; `/cierra` → cabeceras `Connection: close` + cuerpo; `/grande` → 5 MiB. Frente Rust con `handler_timeout` 1 s y `write_timeout` 1 s. Casos: (a) `POST /eco` con `Host: foo.ts.net`, `X-Forwarded-For: 100.64.0.9`, `X-Comandos-Token: <token>`, cuerpo `{"a":1}` → el falso recibió exactamente esas cabeceras (sin XFF añadida, Host intacto, sin `via`), el cuerpo byte a byte y la ruta con query `?x=1`; (b) `GET /lento` responde 200 completo pese al `handler_timeout` de 1 s; (c) sin heredado → 502 con el JSON exacto y `Cache-Control: no-store`, en < 1 s; (d) `/grande` llega íntegro (5 MiB); (e) `/cierra` conserva `Connection: close`; (f) remoto sin token → 401 del frente, el falso **no** recibe nada.
- [ ] **Step 2: Test con el Python real** (`dash_forward_python.rs`): si `python3 -c "import sys"` falla, imprimir «sin python3: prueba saltada» y volver (no `#[ignore]`, para que corra en esta máquina); si no, `HOME` temporal con `H/app-tabs.json = {}` y `H/dash-token` fijo, lanzar `python3 <repo>/bin/cc-dash <puerto libre> --no-open` (como `tests/dash_harness.py:88-135`: `TMUX_TMPDIR` privado, sin `$TMUX`), esperar a que responda, lanzar el frente Rust apuntando a él, y comparar para `GET /prefs`, `GET /notices`, `POST /pane/type` (cuerpo `{}` → 400 del Python) y `GET /no-existe-api` el status, `Content-Type`, `Content-Length` y cuerpo entre pedir al Python directo y pedir al frente, con y sin token remoto (`X-Forwarded-For: 100.64.0.9`, Host `test.ts.net`). Deben ser idénticos.
- [ ] **Step 3: Implementar `forward.rs`** y el campo `Request.body`. **Step 4: Verde.** **Step 5: Commit** `feat(dash): las rutas aún no nativas se reenvían al cc-dash heredado con cabeceras y cuerpo intactos`.

---

### Task 5: `xtask parity` — arnés diferencial Python ↔ Rust

**Files:**
- Create: `xtask/src/parity.rs`, `xtask/parity/README.md`, `xtask/parity/frente.jsonl` (fixture de esta fase: estáticos, puerta, 404, reenvío)
- Modify: `xtask/src/main.rs` (subcomando `parity`), `xtask/Cargo.toml` (ya tiene `serde_json`; añadir `sha2` si se usa; ningún crate de HTTP: el cliente es `std::net::TcpStream` con HTTP/1.1 escrito a mano, suficiente para peticiones de prueba)
- Test: `xtask/tests/parity_normalize.rs` (normalización y comparación, sin servidores)

**Interfaces:**
- CLI: `cargo run -p xtask -- parity --fixture xtask/parity/frente.jsonl --hooks <ruta a ~/.claude/hooks o copia> [--keep]`. Salida: tabla por petición (`OK`/`DIFF`/`SKIP`) y resumen; código 1 si hay `DIFF`. Escribe los pares que difieren en `<scratch>/parity-<fecha>/<n>.{py,rs}` para `diff`.
- Fixture JSONL: `{"name":"state","method":"GET","path":"/state","headers":{"Host":"127.0.0.1"},"body":null,"volatile":["/generated_at","/panes/*/last_seen"],"expect":"same"|"static-accepted"|"skip"}`. `volatile` son JSON pointers (con `*` para índices) que se sustituyen por `"<volátil>"` en ambos lados antes de comparar; `static-accepted` compara status y cuerpo pero no cabeceras ni `Content-Type` exacto (diferencias aceptadas del ruling 2); `skip` registra pero no compara.

Procedimiento: (1) copia `--hooks` a dos directorios temporales (`cp -a`, sin seguir symlinks; `dash/` se deja como symlinks al repo, igual que en producción); (2) `HOME=<copia1>` → `python3 bin/cc-dash <p1> --no-open`; `HOME=<copia2>` → `comandos dash <p2> --legacy-port <puerto muerto>` (así una ruta reenviada da 502 y se marca `SKIP (reenviada)`: en esta fase todo lo no estático es reenviado); ambos con `TMUX_TMPDIR` privado y `tmux -L comandos-test` (variable `TMUX_TMPDIR` basta: ningún `tmux` del arnés ve el servidor del usuario) y `COMANDOS_DASH_DIR=<repo>/dash` para no depender de `H/dash`; (3) por cada petición del fixture, la misma petición a ambos, normalizar, comparar; (4) matar ambos; (5) guardia: aborta antes de lanzar nada si `HOME` de los hijos no empieza por `std::env::temp_dir()` o si `--hooks` apunta a algo que **no** sea un directorio con `state/` dentro.

- [ ] **Step 1: Tests de normalización** (`parity_normalize.rs`): `normalize(json, volatile)` sustituye `/a/b`, `/arr/*/t`; `compare(a, b, expect) -> Outcome` para `same` (status+headers relevantes `content-type`, `content-length`, `cache-control`+cuerpo), `static-accepted` (status+cuerpo), cuerpo no JSON se compara como bytes.
- [ ] **Step 2: Implementar `parity.rs`** y el fixture `frente.jsonl` con ≥ 12 peticiones: `GET /`, `GET /index.html`, `GET /notifications.js`, `GET /assets/xterm/xterm.css` (static-accepted), `GET /vendor/` (static-accepted: 404 en Rust frente a listado HTML en Python → aquí `expect: "skip"` con la razón), `GET /no-existe.css`, `GET /state` sin token local (reenviada → SKIP), `GET /state` remoto sin token (same: 401 idéntico, la puerta responde antes de reenviar), `POST /pane/type` con cuerpo no objeto (same: 400 «El cuerpo debe ser un objeto JSON»), `POST /x` con `Content-Length` 20 000 001 (same: 413 con cierre), `GET /prefs` con `Origin` ajeno (same: 403 «Origen no permitido»), `GET /state` con `Host: evil.com` (same: 403 «Host no permitido»).
- [ ] **Step 3: Correr contra una copia real**: `cargo run -p xtask -- parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks` (el arnés copia; **solo lectura del original**). Pegar el resumen en el reporte. **Step 4: Commit** `feat(xtask): parity compara comandos dash contra cc-dash sobre copias de ~/.claude/hooks`.

---

### Task 6: Unidad heredada, alias y procedimiento de cutover

**Files:**
- Create: `systemd/cc-dash-legacy.service`
- Modify: `systemd/cc-dash.service` (añadir `MemoryMax=2G`, `ManagedOOMPreference=avoid`? **No**: la unidad del frente es la misma `cc-dash.service` y sigue ejecutando `%h/.local/bin/cc-dash 4777 --no-open`; cuando el symlink apunte al Rust, es el Rust. No se cambia en este plan)
- Create: `docs/verification/cutover-dash.md`
- Modify: `crates/comandos-cli/src/install.rs` — nada nuevo: `install --link cc-dash` ya enlaza `~/.local/bin/cc-dash` (archivo real → se mueve a `rollback/cc-dash.orig`? **Hoy es un symlink al repo**: el registro es `LINK:<repo>/bin/cc-dash`, Task 6 de la Fase 1)

```ini
# systemd/cc-dash-legacy.service
[Unit]
Description=Centro Claude heredado (Python) detrás de comandos dash
After=default.target

[Service]
ExecStart=/usr/bin/python3 %h/codebase/0xJesus/ComandOS/bin/cc-dash 4781 --no-open
Restart=on-failure
RestartSec=3
Environment=PYTHONUNBUFFERED=1

[Install]
WantedBy=default.target
```

`cutover-dash.md` documenta, para el controlador: (1) `systemctl --user enable --now cc-dash-legacy.service` y comprobar `curl -s 127.0.0.1:4781/prefs`; (2) sombra: `comandos dash 4782 --legacy-port 4781` en una terminal del controlador + `xtask parity` + navegación manual en `http://127.0.0.1:4782` (índice, notificaciones, uso) en `chrome-bg`; (3) cutover: `comandos install --stage && comandos install --link cc-dash && systemctl --user restart cc-dash.service` (≈ 3 s sin tablero; `cc-app` reintenta solo; tmux y sesiones intactos); (4) verificación: journal del frente sin errores, `tailscale serve status` sin cambios, tablero remoto desde el teléfono; (5) reversión: `comandos install --rollback cc-dash && systemctl --user restart cc-dash.service` (y `disable --now cc-dash-legacy.service` si se abandona), o `comandos install --rollback-release` si el fallo es del binario. Incluye la lista de **diferencias aceptadas** (ruling 2 y HEAD) y la de rutas sin llamador (ruling 3, aún reenviadas).

- [ ] **Step 1: Escribir unidad y doc. Step 2: `systemd-analyze --user verify systemd/cc-dash-legacy.service` (solo verifica el archivo, no instala). Step 3: Commit** `docs(verification): procedimiento de cutover del frente comandos dash y unidad heredada en 4781`.

---

### Task 7: `xtask poll` — carga sintética y RSS

**Files:**
- Create: `xtask/src/poll.rs`
- Modify: `xtask/src/main.rs`
- Test: `xtask/tests/poll_schedule.rs`

**Interfaces:**
- CLI: `cargo run -p xtask -- poll --base http://127.0.0.1:<p> --token <t> --minutes N --pid <pid>` → reproduce el calendario del inventario §1.11 (tabla: `/state` 2 s, `/usage/state` 10 s, `/prefs` 5 s, `/active-tab` 1 s, `/analytics/week` 60 s, `/notices` 3 s, `/notices/watch` long-poll continuo, `/work-marks` 5 s, `/pomodoro` 15 s, `POST /presence` 30 s, `POST /terminal-panes` + `GET /tab-models` 2 s, `GET /workspace` 2 s) con dos clientes (tablero + app), y cada 60 s anota en `docs/verification/rss.jsonl`-compatible (`{"cmd":["dash-poll",…],"minute":n,"pss_kib":…,"rss_kib":…}`) el Pss/RSS de `--pid` leyendo `/proc/<pid>/smaps_rollup` (reusar `rss.rs`). Imprime al final el mínimo, máximo y la pendiente (kB/h) entre el minuto 5 y el último.
- Test: `schedule(minutes) -> Vec<(instant_ms, method, path)>` produce las frecuencias exactas de la tabla en 2 min (p. ej. `/state` 60 veces, `/active-tab` 120, `/analytics/week` 2).

Se usa en la 2b como criterio de memoria por dominio (y aquí para medir el frente reenviando: su RSS debe quedar plano; el crecimiento, si lo hay, está en el Python).

- [ ] **Step 1: Test del calendario. Step 2: Implementar. Step 3: Correr 3 minutos contra el frente de sombra con el Python detrás y pegar el resumen. Step 4: Commit** `feat(xtask): poll reproduce la carga real del tablero y mide Pss del servidor`.

---

## Self-review

- Spec §4.2: 4777, misma puerta (Task 2), estáticos (Task 3; embebido diferido por ruling 2), estado en SQLite y SSE → Fase 2b/3 (enmiendas). §5.2 sombra y arnés (Task 5), §5.3 cutover por symlink y restart de su unidad (Task 6), §5.4 reversión (Task 1 + Task 6), §5.5 criterio: arnés en verde, RSS medido (Task 7), sesiones intactas (Global Constraints). §6 `xtask parity`/`bench` (Tasks 5 y 7). Important 8 de la revisión final (Task 1).
- Tipos: `DashConfig`, `RouteClass`, `Request.body`, `statics::serve`, `forward::relay` nombrados igual en 2, 3, 4 y 5.
- Sin placeholders salvo los dos `todo!()` de Task 1 que el mismo paso ordena escribir; los tests de Task 3 y 4 están descritos caso a caso con valores exactos.
