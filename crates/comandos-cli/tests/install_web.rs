// crates/comandos-cli/tests/install_web.rs
//! `install --stage` con `web/` junto al binario (T4, preflight R14). Cada prueba
//! copia el binario a un directorio de origen propio y usa un HOME temporal.
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cmd-web-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// `<root>/bin/comandos` (copia del binario de prueba) y `<root>/web` opcional:
/// `../web` relativo al ejecutable, como `target/release/comandos` y `target/web`.
fn source(root: &Path) -> PathBuf {
    let bin = root.join("bin/comandos");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_comandos"), &bin).unwrap();
    bin
}

fn stage(exe: &Path, home: &Path, web_env: Option<&Path>) -> Output {
    let mut cmd = Command::new(exe);
    cmd.args(["install", "--home"])
        .arg(home)
        .arg("--stage")
        .env_remove("COMANDOS_WEB_SOURCE");
    if let Some(w) = web_env {
        cmd.env("COMANDOS_WEB_SOURCE", w);
    }
    cmd.output().unwrap()
}

fn ok(o: &Output) {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

fn sha12(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))[..12].to_string()
}

fn releases(home: &Path) -> PathBuf {
    home.join(".local/share/comandos/releases")
}

fn current(home: &Path) -> String {
    let link = fs::read_link(home.join(".local/share/comandos/bin/comandos")).unwrap();
    link.parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

fn write_web(web: &Path, boot: &str) {
    fs::create_dir_all(web.join("abc")).unwrap();
    fs::write(web.join("abc/boot.js"), boot).unwrap();
    fs::write(web.join("manifest.json"), r#"{"files":{}}"#).unwrap();
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn stage_copies_web_next_to_the_binary_into_the_release() {
    let src = scratch("src");
    let home = scratch("home");
    let exe = source(&src);
    write_web(&src.join("web"), "// boot\n");
    // Permisos de origen raros: la release los normaliza a 0644/0755.
    fs::set_permissions(
        src.join("web/abc/boot.js"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    fs::set_permissions(src.join("web/abc"), fs::Permissions::from_mode(0o700)).unwrap();
    ok(&stage(&exe, &home, None));
    let id = current(&home);
    let rel = releases(&home).join(&id);
    assert_eq!(
        fs::read_to_string(rel.join("web/abc/boot.js")).unwrap(),
        "// boot\n"
    );
    assert!(rel.join("web/manifest.json").is_file());
    assert_eq!(mode(&rel.join("web/abc/boot.js")), 0o644);
    assert_eq!(mode(&rel.join("web/manifest.json")), 0o644);
    assert_eq!(mode(&rel.join("web")), 0o755);
    assert_eq!(mode(&rel.join("web/abc")), 0o755);
    assert_eq!(mode(&rel.join("comandos")) & 0o111, 0o111);
    // Con `web/`, el id ya no es el sha12 del binario solo.
    assert_ne!(id, sha12(&exe));
    // Sin restos de la preparación.
    let leftovers: Vec<_> = fs::read_dir(releases(&home))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn without_web_the_release_id_is_the_binary_sha12_as_today() {
    let src = scratch("noweb-src");
    let home = scratch("noweb-home");
    let exe = source(&src);
    ok(&stage(&exe, &home, None));
    let id = current(&home);
    assert_eq!(id, sha12(&exe));
    assert!(!releases(&home).join(&id).join("web").exists());
}

#[test]
fn a_changed_web_with_the_same_binary_is_a_new_release_and_rollback_works() {
    let src = scratch("chg-src");
    let home = scratch("chg-home");
    let exe = source(&src);
    write_web(&src.join("web"), "// v1\n");
    ok(&stage(&exe, &home, None));
    let v1 = current(&home);
    fs::write(src.join("web/abc/boot.js"), "// v2\n").unwrap();
    ok(&stage(&exe, &home, None));
    let v2 = current(&home);
    assert_ne!(v1, v2, "web distinto con el mismo binario: release nueva");
    assert_eq!(
        fs::read_to_string(releases(&home).join(&v2).join("web/abc/boot.js")).unwrap(),
        "// v2\n"
    );
    // La release vieja conserva su web.
    assert_eq!(
        fs::read_to_string(releases(&home).join(&v1).join("web/abc/boot.js")).unwrap(),
        "// v1\n"
    );
    assert_eq!(
        fs::read_to_string(releases(&home).join("previous"))
            .unwrap()
            .trim(),
        v1
    );
    // Mismo web otra vez: misma release (idempotente).
    ok(&stage(&exe, &home, None));
    assert_eq!(current(&home), v2);
    // Rollback por id a la anterior.
    let out = Command::new(&exe)
        .args(["install", "--home"])
        .arg(&home)
        .arg("--rollback-release")
        .output()
        .unwrap();
    ok(&out);
    assert_eq!(current(&home), v1);
}

#[test]
fn rollback_between_an_old_binary_only_release_and_a_web_release() {
    // Una release de hoy (sin web, id = sha12) sigue siendo destino de rollback.
    let src = scratch("mix-src");
    let home = scratch("mix-home");
    let exe = source(&src);
    ok(&stage(&exe, &home, None));
    let plain = current(&home);
    assert_eq!(plain, sha12(&exe));
    write_web(&src.join("web"), "// web\n");
    ok(&stage(&exe, &home, None));
    let with_web = current(&home);
    assert_ne!(with_web, plain);
    let out = Command::new(&exe)
        .args(["install", "--home"])
        .arg(&home)
        .arg("--rollback-release")
        .output()
        .unwrap();
    ok(&out);
    assert_eq!(current(&home), plain);
    assert!(!releases(&home).join(&plain).join("web").exists());
}

#[test]
fn comandos_web_source_overrides_and_empty_disables() {
    let src = scratch("env-src");
    let home = scratch("env-home");
    let exe = source(&src);
    write_web(&src.join("web"), "// junto\n");
    let other = scratch("env-other");
    write_web(&other, "// explícito\n");
    ok(&stage(&exe, &home, Some(&other)));
    let id = current(&home);
    assert_eq!(
        fs::read_to_string(releases(&home).join(&id).join("web/abc/boot.js")).unwrap(),
        "// explícito\n"
    );
    // Vacío: sin web aunque exista `../web`.
    let home2 = scratch("env-home2");
    ok(&stage(&exe, &home2, Some(Path::new(""))));
    assert_eq!(current(&home2), sha12(&exe));
    // Explícito pero ausente: error, no se instala sin web en silencio.
    let home3 = scratch("env-home3");
    let out = stage(&exe, &home3, Some(&src.join("no-existe")));
    assert_eq!(out.status.code(), Some(1));
    assert!(!home3.join(".local/share/comandos/bin/comandos").exists());
}

#[test]
fn symlinks_inside_web_are_rejected() {
    let src = scratch("lnk-src");
    let home = scratch("lnk-home");
    let exe = source(&src);
    write_web(&src.join("web"), "// boot\n");
    symlink("/etc/passwd", src.join("web/abc/passwd")).unwrap();
    let out = stage(&exe, &home, None);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("enlace simbólico"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!home.join(".local/share/comandos/bin/comandos").exists());
    // Sin restos de la preparación ni release a medias.
    let rel = releases(&home);
    if rel.exists() {
        assert_eq!(fs::read_dir(&rel).unwrap().count(), 0);
    }
}

#[test]
fn restaging_from_an_installed_release_keeps_its_web_and_id() {
    let src = scratch("re-src");
    let home = scratch("re-home");
    let exe = source(&src);
    write_web(&src.join("web"), "// boot\n");
    ok(&stage(&exe, &home, None));
    let id = current(&home);
    // `releases/<id>/comandos install --stage`: su `web/` es el hermano del binario.
    let installed = releases(&home).join(&id).join("comandos");
    ok(&stage(&installed, &home, None));
    assert_eq!(current(&home), id);
}
