// crates/comandos-cli/tests/install_web.rs
//! `install --stage` con `web/` (T4, preflight R14). El origen es siempre explícito
//! (`--web` o `COMANDOS_WEB_SOURCE`), salvo re-instalar desde una release instalada.
//! Cada prueba copia el binario a un directorio de origen propio y usa un HOME temporal.
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, SystemTime},
};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cmd-web-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// `<root>/bin/comandos`: copia del binario de prueba.
fn source(root: &Path) -> PathBuf {
    let bin = root.join("bin/comandos");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_comandos"), &bin).unwrap();
    bin
}

/// `install --home H --stage [extra…]`, sin `COMANDOS_WEB_SOURCE` salvo `env`.
fn stage_with(exe: &Path, home: &Path, extra: &[&Path], env: Option<&Path>) -> Output {
    let mut cmd = Command::new(exe);
    cmd.args(["install", "--home"])
        .arg(home)
        .arg("--stage")
        .env_remove("COMANDOS_WEB_SOURCE");
    if let Some((flag, rest)) = extra.split_first() {
        cmd.arg(flag);
        cmd.args(rest);
    }
    if let Some(w) = env {
        cmd.env("COMANDOS_WEB_SOURCE", w);
    }
    run(&mut cmd)
}

/// `Command::output` reintentando ETXTBSY: otra prueba que hace `fork` mientras se
/// copia el binario hereda un instante su descriptor de escritura y `exec` falla.
fn run(cmd: &mut Command) -> Output {
    for _ in 0..100 {
        match cmd.output() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other.unwrap(),
        }
    }
    panic!("ETXTBSY persistente");
}

fn stage(exe: &Path, home: &Path, web: Option<&Path>) -> Output {
    match web {
        Some(w) => stage_with(exe, home, &[Path::new("--web"), w], None),
        None => stage_with(exe, home, &[], None),
    }
}

fn ok(o: &Output) -> String {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn fails(o: &Output) -> String {
    assert_eq!(
        o.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn sha12(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))[..12].to_string()
}

fn releases(home: &Path) -> PathBuf {
    home.join(".local/share/comandos/releases")
}

fn bin(home: &Path) -> PathBuf {
    home.join(".local/share/comandos/bin/comandos")
}

fn current(home: &Path) -> String {
    let link = fs::read_link(bin(home)).unwrap();
    link.parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

/// `web/` válido: `abc/boot.js` y un manifiesto que lo referencia.
fn write_web(web: &Path, boot: &str) {
    fs::create_dir_all(web.join("abc")).unwrap();
    fs::write(web.join("abc/boot.js"), boot).unwrap();
    fs::write(
        web.join("manifest.json"),
        r#"{"files":{"x_boot.js":"abc/boot.js"}}"#,
    )
    .unwrap();
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

fn rollback(exe: &Path, home: &Path) -> Output {
    run(Command::new(exe)
        .args(["install", "--home"])
        .arg(home)
        .arg("--rollback-release"))
}

/// Lista recursiva (ruta relativa, bytes) para comparar un árbol antes y después.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            if p.symlink_metadata().unwrap().is_dir() {
                out.push((rel, Vec::new()));
                todo.push(p);
            } else {
                out.push((rel, fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn stage_with_web_copies_it_into_the_release_and_prints_the_source() {
    let src = scratch("src");
    let home = scratch("home");
    let exe = source(&src);
    let web = src.join("artefactos");
    write_web(&web, "// boot\n");
    // Permisos de origen raros: la release los normaliza a 0644/0755.
    fs::set_permissions(web.join("abc/boot.js"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(web.join("abc"), fs::Permissions::from_mode(0o700)).unwrap();
    let out = ok(&stage(&exe, &home, Some(&web)));
    let id = current(&home);
    assert_eq!(
        out.trim(),
        format!(
            "release {id} (web: {}, 2 archivos)",
            fs::canonicalize(&web).unwrap().display()
        )
    );
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
    assert_ne!(id, sha12(&exe));
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
    let out = ok(&stage(&exe, &home, None));
    let id = current(&home);
    assert_eq!(id, sha12(&exe));
    assert_eq!(out.trim(), format!("release {id} (sin web)"));
    assert!(!releases(&home).join(&id).join("web").exists());
}

#[test]
fn no_implicit_web_next_to_the_binary_or_its_parent() {
    // `../web` (como `target/web`) y `web/` hermano NO se toman sin `--web`.
    let src = scratch("impl-src");
    let home = scratch("impl-home");
    let exe = source(&src);
    write_web(&src.join("web"), "// padre\n");
    write_web(&src.join("bin/web"), "// hermano\n");
    let out = ok(&stage(&exe, &home, None));
    assert_eq!(current(&home), sha12(&exe));
    assert!(out.contains("(sin web)"), "{out}");
    assert!(!releases(&home).join(sha12(&exe)).join("web").exists());
}

#[test]
fn a_changed_web_with_the_same_binary_is_a_new_release_and_rollback_works() {
    let src = scratch("chg-src");
    let home = scratch("chg-home");
    let exe = source(&src);
    let web = src.join("web");
    write_web(&web, "// v1\n");
    ok(&stage(&exe, &home, Some(&web)));
    let v1 = current(&home);
    fs::write(web.join("abc/boot.js"), "// v2\n").unwrap();
    ok(&stage(&exe, &home, Some(&web)));
    let v2 = current(&home);
    assert_ne!(v1, v2, "web distinto con el mismo binario: release nueva");
    assert_eq!(
        fs::read_to_string(releases(&home).join(&v2).join("web/abc/boot.js")).unwrap(),
        "// v2\n"
    );
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
    ok(&stage(&exe, &home, Some(&web)));
    assert_eq!(current(&home), v2);
    ok(&rollback(&exe, &home));
    assert_eq!(current(&home), v1);
}

#[test]
fn rollback_between_an_old_binary_only_release_and_a_web_release() {
    let src = scratch("mix-src");
    let home = scratch("mix-home");
    let exe = source(&src);
    ok(&stage(&exe, &home, None));
    let plain = current(&home);
    assert_eq!(plain, sha12(&exe));
    let web = src.join("web");
    write_web(&web, "// web\n");
    ok(&stage(&exe, &home, Some(&web)));
    let with_web = current(&home);
    assert_ne!(with_web, plain);
    ok(&rollback(&exe, &home));
    assert_eq!(current(&home), plain);
    assert!(!releases(&home).join(&plain).join("web").exists());
}

#[test]
fn comandos_web_source_works_and_the_flag_wins() {
    let src = scratch("env-src");
    let home = scratch("env-home");
    let exe = source(&src);
    let by_env = scratch("env-a");
    write_web(&by_env, "// env\n");
    let by_flag = scratch("env-b");
    write_web(&by_flag, "// flag\n");
    ok(&stage_with(&exe, &home, &[], Some(&by_env)));
    let id = current(&home);
    assert_eq!(
        fs::read_to_string(releases(&home).join(&id).join("web/abc/boot.js")).unwrap(),
        "// env\n"
    );
    ok(&stage_with(
        &exe,
        &home,
        &[Path::new("--web"), &by_flag],
        Some(&by_env),
    ));
    let id = current(&home);
    assert_eq!(
        fs::read_to_string(releases(&home).join(&id).join("web/abc/boot.js")).unwrap(),
        "// flag\n"
    );
    // Vacío: sin web.
    let home2 = scratch("env-home2");
    ok(&stage_with(&exe, &home2, &[], Some(Path::new(""))));
    assert_eq!(current(&home2), sha12(&exe));
    // Explícito pero ausente: error, no se instala nada.
    let home3 = scratch("env-home3");
    let err = fails(&stage(&exe, &home3, Some(&src.join("no-existe"))));
    assert!(err.contains("no-existe"), "{err}");
    assert!(!bin(&home3).exists());
}

#[test]
fn web_flag_is_only_valid_with_stage() {
    let src = scratch("flag-src");
    let home = scratch("flag-home");
    let exe = source(&src);
    let out = run(Command::new(&exe)
        .args(["install", "--home"])
        .arg(&home)
        .args(["--releases", "--web", "/x"]));
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn web_without_a_valid_manifest_is_refused_and_names_the_path() {
    let src = scratch("man-src");
    let home = scratch("man-home");
    let exe = source(&src);
    // Vacío.
    let empty = scratch("man-empty");
    let err = fails(&stage(&exe, &home, Some(&empty)));
    assert!(
        err.contains(&empty.join("manifest.json").display().to_string()),
        "{err}"
    );
    // Sin manifiesto (p. ej. un web-build que falló a medias).
    let partial = scratch("man-partial");
    fs::create_dir_all(partial.join("abcdef012345")).unwrap();
    fs::write(partial.join("abcdef012345/a.wasm"), b"\0asm").unwrap();
    let err = fails(&stage(&exe, &home, Some(&partial)));
    assert!(err.contains("manifest.json"), "{err}");
    // JSON que no es un Manifest.
    let junk = scratch("man-junk");
    fs::write(junk.join("manifest.json"), b"[1,2]").unwrap();
    let err = fails(&stage(&exe, &home, Some(&junk)));
    assert!(err.contains("manifest.json"), "{err}");
    // Referencia a un archivo que no está.
    let dangling = scratch("man-dangling");
    write_web(&dangling, "// boot\n");
    fs::remove_file(dangling.join("abc/boot.js")).unwrap();
    let err = fails(&stage(&exe, &home, Some(&dangling)));
    assert!(err.contains("abc/boot.js"), "{err}");
    // Ruta que sale del directorio.
    let escape = scratch("man-escape");
    fs::write(
        escape.join("manifest.json"),
        r#"{"files":{"x.js":"../x.js"}}"#,
    )
    .unwrap();
    let err = fails(&stage(&exe, &home, Some(&escape)));
    assert!(err.contains("../x.js"), "{err}");
    assert!(!bin(&home).exists());
    assert!(!releases(&home).exists() || fs::read_dir(releases(&home)).unwrap().count() == 0);
}

#[test]
fn a_refused_web_never_touches_the_live_release() {
    let src = scratch("live-src");
    let home = scratch("live-home");
    let exe = source(&src);
    ok(&stage(&exe, &home, None));
    let live = current(&home);
    let before = snapshot(&releases(&home));
    let empty = scratch("live-empty");
    fails(&stage(&exe, &home, Some(&empty)));
    assert_eq!(current(&home), live);
    assert_eq!(snapshot(&releases(&home)), before);
    assert!(!releases(&home).join(&live).join("web").exists());
}

#[test]
fn symlinks_inside_web_are_rejected() {
    let src = scratch("lnk-src");
    let home = scratch("lnk-home");
    let exe = source(&src);
    let web = src.join("web");
    write_web(&web, "// boot\n");
    symlink("/etc/passwd", web.join("abc/passwd")).unwrap();
    let err = fails(&stage(&exe, &home, Some(&web)));
    assert!(err.contains("enlace simbólico"), "{err}");
    assert!(!bin(&home).exists());
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
    let web = src.join("web");
    write_web(&web, "// boot\n");
    ok(&stage(&exe, &home, Some(&web)));
    let id = current(&home);
    // `releases/<id>/comandos install --stage`: su `web/` es de esa release.
    let installed = releases(&home).join(&id).join("comandos");
    let out = ok(&stage(&installed, &home, None));
    assert_eq!(current(&home), id);
    assert!(out.contains(&format!("release {id} (web: ")), "{out}");
}

#[test]
fn restaging_from_a_tampered_release_is_refused() {
    let src = scratch("tam-src");
    let home = scratch("tam-home");
    let exe = source(&src);
    let web = src.join("web");
    write_web(&web, "// v1\n");
    ok(&stage(&exe, &home, Some(&web)));
    let id = current(&home);
    let dir = releases(&home).join(&id);
    fs::write(dir.join("web/abc/boot.js"), "// alterado\n").unwrap();
    let before = snapshot(&releases(&home));
    let err = fails(&stage(&dir.join("comandos"), &home, None));
    assert!(err.contains(&id), "{err}");
    assert_eq!(current(&home), id);
    assert_eq!(snapshot(&releases(&home)), before);
}

#[test]
fn stage_prunes_abandoned_staging_and_half_releases() {
    let src = scratch("prn-src");
    let home = scratch("prn-home");
    let exe = source(&src);
    let rel = releases(&home);
    let old = SystemTime::now() - Duration::from_secs(3 * 3600);
    let age = |p: &Path| {
        fs::File::open(p).unwrap().set_modified(old).unwrap();
    };
    for d in [".web-stage.99999", "0123456789ab"] {
        fs::create_dir_all(rel.join(d).join("web")).unwrap();
        age(&rel.join(d));
    }
    // Recientes: pueden ser de otro `--stage` en curso; se respetan.
    for d in [".web-stage.88888", "ba9876543210"] {
        fs::create_dir_all(rel.join(d).join("web")).unwrap();
    }
    ok(&stage(&exe, &home, None));
    assert!(!rel.join(".web-stage.99999").exists());
    assert!(!rel.join("0123456789ab").exists());
    assert!(rel.join(".web-stage.88888").exists());
    assert!(rel.join("ba9876543210").exists());
}

#[test]
fn a_reused_half_release_with_a_tampered_web_is_never_linked() {
    // `<id>/` a medias (sin `comandos`) que dejó un `--stage` cortado, con su `web/`
    // alterado después: el siguiente `--stage` del mismo contenido no la enlaza.
    let src = scratch("half-src");
    let exe = source(&src);
    let web = src.join("web");
    write_web(&web, "// boot\n");
    let first = scratch("half-a");
    ok(&stage(&exe, &first, Some(&web)));
    let id = current(&first);
    let home = scratch("half-b");
    let half = releases(&home).join(&id);
    fs::create_dir_all(half.join("web/abc")).unwrap();
    for f in ["manifest.json", "abc/boot.js"] {
        fs::copy(
            releases(&first).join(&id).join("web").join(f),
            half.join("web").join(f),
        )
        .unwrap();
    }
    fs::write(half.join("web/abc/boot.js"), "// alterado\n").unwrap();
    let err = fails(&stage(&exe, &home, Some(&web)));
    assert!(err.contains(&id), "{err}");
    assert!(!bin(&home).exists());
}

#[test]
fn repeated_web_flag_is_a_usage_error_and_the_printed_path_is_canonical() {
    let src = scratch("rep-src");
    let home = scratch("rep-home");
    let exe = source(&src);
    let web = src.join("web");
    write_web(&web, "// boot\n");
    let out = stage_with(
        &exe,
        &home,
        &[Path::new("--web"), &web, Path::new("--web"), &web],
        None,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(!bin(&home).exists());
    // Ruta con `..`: se imprime la canónica.
    let twisted = src.join("bin/../web");
    let out = ok(&stage(&exe, &home, Some(&twisted)));
    let canon = fs::canonicalize(&web).unwrap();
    assert!(
        out.contains(&format!("(web: {}, 2 archivos)", canon.display())),
        "{out}"
    );
}
