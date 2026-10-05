use comandos_app::config::{
    AppConfig, Entry, RunMode, TmuxServer, parse_args, resolve_entry, ui_lang,
};
use std::path::PathBuf;
use std::process::Command;

fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |k| {
        pairs
            .iter()
            .find(|(a, _)| *a == k)
            .map(|(_, v)| v.to_string())
    }
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

const SBX_ENV: &[(&str, &str)] = &[("HOME", "/h"), ("XDG_RUNTIME_DIR", "/run/user/1000")];

#[test]
fn entry_follows_argv0() {
    assert_eq!(
        resolve_entry("/home/u/.local/bin/cc-app"),
        Entry::App { default_live: true }
    );
    assert_eq!(
        resolve_entry("comandos-app"),
        Entry::App {
            default_live: false
        }
    );
    assert_eq!(resolve_entry("/x/cc-notifyd"), Entry::Notifyd);
    assert_eq!(resolve_entry("comandos-notifyd"), Entry::Notifyd);
}

#[test]
fn bare_comandos_app_is_sandbox() {
    let env = env_of(&[
        ("HOME", "/h"),
        ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ("DISPLAY", ":1"),
    ]);
    let cfg = parse_args(&args(&[]), false, &env).expect("config");
    assert_eq!(cfg.mode(), RunMode::Sandbox);
    assert_eq!(cfg.tmux_socket(), Some("comandos-app-sbx"));
    assert_eq!(
        cfg.hooks_dir(),
        PathBuf::from("/run/user/1000/comandos-app-sbx/hooks")
    );
    assert_eq!(cfg.dash_url(), None);
    assert!(cfg.writes_allowed());
}

#[test]
fn cc_app_defaults_to_live_with_real_paths() {
    let env = env_of(&[
        ("HOME", "/h"),
        ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ("DISPLAY", ":1"),
    ]);
    let cfg = parse_args(&args(&[]), true, &env).expect("config");
    assert_eq!(cfg.mode(), RunMode::Live);
    assert_eq!(cfg.tmux_socket(), None);
    assert_eq!(cfg.tmux_server(), TmuxServer::User);
    assert_eq!(cfg.hooks_dir(), PathBuf::from("/h/.claude/hooks"));
    assert_eq!(cfg.dash_url(), Some("http://127.0.0.1:4777"));
    assert_eq!(
        cfg.web_data_dir(),
        PathBuf::from("/h/.local/share/comandos")
    );
    assert_eq!(cfg.web_cache_dir(), PathBuf::from("/h/.cache/comandos"));
    assert_eq!(cfg.lock_file_name(":1"), "cc-app-1.lock");
    assert_eq!(cfg.wm_class(), "comandos");
    assert_eq!(cfg.title(), "ComandOS");
}

#[test]
fn shadow_never_looks_like_the_real_app() {
    let env = env_of(SBX_ENV);
    let cfg: AppConfig = parse_args(&args(&["--mode", "shadow"]), false, &env).expect("config");
    assert_eq!(cfg.mode(), RunMode::Shadow);
    assert!(!cfg.writes_allowed());
    for name in [cfg.title(), cfg.wm_class()] {
        assert!(!name.to_lowercase().contains("comandos"), "{name}");
    }
    assert_eq!(cfg.lock_file_name(":1"), "sombra-app-rs-1.lock");
    assert_eq!(
        cfg.web_data_dir(),
        PathBuf::from("/run/user/1000/comandos-app-shadow/data")
    );
    assert_eq!(
        cfg.layout_dump_path(),
        PathBuf::from("/run/user/1000/comandos-app-shadow-layout.json")
    );
}

#[test]
fn dash_url_must_be_loopback() {
    let env = env_of(&[("HOME", "/h")]);
    let err = parse_args(
        &args(&["--dash-url", "http://example.com:4777"]),
        false,
        &env,
    )
    .unwrap_err();
    assert!(err.contains("127.0.0.1"), "{err}");
}

#[test]
fn unknown_flag_is_usage_error() {
    let env = env_of(&[("HOME", "/h")]);
    assert!(parse_args(&args(&["--modo", "live"]), false, &env).is_err());
    assert!(parse_args(&args(&["--mode"]), false, &env).is_err());
    assert!(parse_args(&args(&["--mode", "real"]), false, &env).is_err());
}

/// F19: el mismo nombre que `bin/cc-app:43`, comparado contra el propio Python.
#[test]
fn lock_file_name_matches_python() {
    let env = env_of(SBX_ENV);
    let live = parse_args(&args(&["--mode", "live"]), false, &env).expect("live");
    let shadow = parse_args(&args(&["--mode", "shadow"]), false, &env).expect("shadow");
    let sandbox = parse_args(&args(&[]), false, &env).expect("sandbox");
    assert_eq!(live.lock_file_name(":0/x"), "cc-app-0_x.lock");
    assert_eq!(shadow.lock_file_name(":0/x"), "sombra-app-rs-0_x.lock");
    assert_eq!(sandbox.lock_file_name(":0/x"), "comandos-app-sbx-0_x.lock");
    for display in [":0", ":1", ":0/x", "", ":", "a/b:c", "host:10.0", "//"] {
        let out = Command::new("python3")
            .arg("-c")
            .arg(
                "import sys; d = sys.argv[1]; \
                 print(f\"cc-app-{(d or 'x').replace('/', '_').replace(':', '')}.lock\")",
            )
            .arg(display)
            .output()
            .expect("python3");
        assert!(out.status.success(), "python3 falló con {display:?}");
        let python = String::from_utf8(out.stdout).expect("utf8");
        assert_eq!(
            live.lock_file_name(display),
            python.trim_end_matches('\n'),
            "{display:?}"
        );
    }
}

/// F20: el sandbox nunca habla con el tablero real; solo la banda de devhost.
#[test]
fn sandbox_dash_url_stays_in_devhost_band() {
    let env = env_of(SBX_ENV);
    for port in ["4777", "4778", "4782", "7199", "7400", "8080", "40000"] {
        let url = format!("http://127.0.0.1:{port}");
        let err = parse_args(&args(&["--dash-url", &url]), false, &env).unwrap_err();
        assert!(err.contains("7200"), "{port}: {err}");
    }
    for port in ["7200", "7311", "7399"] {
        let url = format!("http://127.0.0.1:{port}/");
        let cfg = parse_args(&args(&["--dash-url", &url]), false, &env).expect("banda");
        assert_eq!(
            cfg.dash_url(),
            Some(format!("http://127.0.0.1:{port}").as_str())
        );
    }
    // Fuera del sandbox el puerto real sigue permitido.
    let live = parse_args(
        &args(&["--mode", "live", "--dash-url", "http://127.0.0.1:4778"]),
        false,
        &env,
    )
    .expect("live");
    assert_eq!(live.dash_url(), Some("http://127.0.0.1:4778"));
}

/// El sandbox siempre lleva un servidor tmux propio: ni ausente ni `default`.
#[test]
fn sandbox_always_carries_a_private_socket() {
    let env = env_of(SBX_ENV);
    for bad in ["default", "", "../x", "a/b", "con espacio"] {
        assert!(
            parse_args(&args(&["--tmux-socket", bad]), false, &env).is_err(),
            "{bad:?} aceptado"
        );
    }
    let cfg = parse_args(&args(&["--tmux-socket", "mi-sbx.1"]), false, &env).expect("propio");
    match cfg.tmux_server() {
        TmuxServer::Private(label) => assert_eq!(label.as_str(), "mi-sbx.1"),
        TmuxServer::User => panic!("sandbox sin socket propio"),
    }
    let cfg = parse_args(&args(&[]), false, &env).expect("por omisión");
    assert!(matches!(cfg.tmux_server(), TmuxServer::Private(_)));
    // `default` tampoco vale fuera del sandbox: para el servidor del usuario se omite la opción.
    assert!(
        parse_args(
            &args(&["--mode", "shadow", "--tmux-socket", "default"]),
            false,
            &env
        )
        .is_err()
    );
}

/// F10: el sandbox no puede apuntar sus hooks a los reales.
#[test]
fn sandbox_refuses_real_hooks_dir() {
    let env = env_of(SBX_ENV);
    for bad in [
        "/h/.claude/hooks",
        "/h/.claude/hooks/",
        "/h/.claude/./hooks",
        "/h/.claude/hooks/sub",
    ] {
        assert!(
            parse_args(&args(&["--hooks-dir", bad]), false, &env).is_err(),
            "{bad}"
        );
    }
    let cfg = parse_args(&args(&["--hooks-dir", "/tmp/sbx-hooks"]), false, &env).expect("temporal");
    assert_eq!(cfg.hooks_dir(), PathBuf::from("/tmp/sbx-hooks"));
    assert_eq!(
        cfg.sandbox_root(),
        Some(PathBuf::from("/run/user/1000/comandos-app-sbx").as_path())
    );
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("comandos-app-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temporal");
    dir
}

fn env_owned(pairs: Vec<(&'static str, String)>) -> impl Fn(&str) -> Option<String> {
    move |k| pairs.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone())
}

fn p(path: &std::path::Path) -> String {
    path.to_str().expect("utf8").to_string()
}

/// Ronda 1, I1: en sandbox toda ruta escribible se resuelve y tiene que quedar dentro
/// de `sandbox_root()` o del temporal (`TMPDIR`). Los «estados reales» son fixtures en
/// un temporal propio: la prueba nunca mira el `~/.claude` de verdad.
#[test]
fn sandbox_writes_only_inside_its_roots() {
    let base = scratch("confine");
    let (run, tmp, home, real) = (
        base.join("run"),
        base.join("tmp"),
        base.join("home"),
        base.join("realhome"),
    );
    for dir in [
        &run,
        &tmp,
        &home.join(".claude/hooks"),
        &home.join(".config/comandos"),
        &real.join(".claude/hooks"),
        &tmp.join("dentro"),
    ] {
        std::fs::create_dir_all(dir).expect("fixture");
    }
    // Enlaces dentro del temporal permitido: uno sale a los hooks «reales», otro se queda dentro.
    std::os::unix::fs::symlink(home.join(".claude/hooks"), tmp.join("link")).expect("link");
    std::os::unix::fs::symlink(tmp.join("dentro"), tmp.join("inlink")).expect("inlink");
    // Enlace colgante hacia fuera: crear a través de él escribiría fuera.
    std::os::unix::fs::symlink(home.join(".claude/nuevo"), tmp.join("colgante")).expect("dangling");

    let env = env_owned(vec![
        ("HOME", p(&home)),
        ("XDG_RUNTIME_DIR", p(&run)),
        ("TMPDIR", p(&tmp)),
    ]);
    let bad = [
        // Los hooks reales con HOME apuntando a otro sitio.
        real.join(".claude/hooks"),
        // Un subdirectorio que no existe, alcanzado por un enlace que sale.
        tmp.join("link/nueva-sub"),
        tmp.join("link"),
        // `..` tras un enlace (el núcleo lo resuelve a ~/.claude/x).
        tmp.join("link/../x"),
        // `..` en la parte que no existe.
        tmp.join("no-existe/../x"),
        tmp.join("../home/.claude"),
        tmp.join("colgante"),
        tmp.join("colgante/sub"),
        home.join(".claude/hooks"),
        home.join(".claude"),
        home.clone(),
        home.join(".config/comandos"),
        base.join("fuera"),
        PathBuf::from("/"),
    ];
    for path in &bad {
        let err = parse_args(&args(&["--hooks-dir", &p(path)]), false, &env)
            .expect_err(&format!("aceptado: {}", path.display()));
        assert!(err.contains("sandbox"), "{err}");
    }
    let canon_tmp = std::fs::canonicalize(&tmp).expect("tmp");
    let canon_run = std::fs::canonicalize(&run).expect("run");
    let good = [
        (tmp.join("sbx-hooks"), canon_tmp.join("sbx-hooks")),
        (tmp.join("inlink/sub"), canon_tmp.join("dentro/sub")),
        (tmp.join("./a/b"), canon_tmp.join("a/b")),
        (
            run.join("comandos-app-sbx/otros"),
            canon_run.join("comandos-app-sbx/otros"),
        ),
    ];
    for (path, want) in &good {
        let cfg = parse_args(&args(&["--hooks-dir", &p(path)]), false, &env)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(cfg.hooks_dir(), want.as_path());
    }
    // Por omisión todo lo escribible cae dentro de la raíz del sandbox.
    let cfg = parse_args(&args(&[]), false, &env).expect("omisión");
    let root = canon_run.join("comandos-app-sbx");
    assert_eq!(cfg.sandbox_root(), Some(root.as_path()));
    for path in [
        cfg.hooks_dir().to_path_buf(),
        cfg.web_data_dir().to_path_buf(),
        cfg.web_cache_dir().to_path_buf(),
        cfg.layout_dump_path(),
    ] {
        assert!(path.starts_with(&root), "{}", path.display());
    }
    // Un TMPDIR que contiene el HOME no cuenta como raíz permitida.
    let env = env_owned(vec![
        ("HOME", p(&home)),
        ("XDG_RUNTIME_DIR", p(&run)),
        ("TMPDIR", p(&base)),
    ]);
    assert!(parse_args(&args(&["--hooks-dir", &p(&base.join("x"))]), false, &env).is_err());
    // Ni una raíz de sandbox que contenga el HOME.
    let env = env_owned(vec![
        ("HOME", p(&run.join("comandos-app-sbx/home"))),
        ("XDG_RUNTIME_DIR", p(&run)),
        ("TMPDIR", p(&tmp)),
    ]);
    assert!(parse_args(&args(&[]), false, &env).is_err());
    let _ = std::fs::remove_dir_all(&base);
}

/// La sombra nunca escribe; sandbox y live sí.
#[test]
fn only_shadow_forbids_writes() {
    let env = env_of(SBX_ENV);
    let mode = |m: &str| parse_args(&args(&["--mode", m]), false, &env).expect(m);
    assert!(!mode("shadow").writes_allowed());
    assert!(mode("sandbox").writes_allowed());
    assert!(mode("live").writes_allowed());
}

/// `COMANDOS_DASH_URL`: vacío = sin definir; se lee en sombra/live, nunca en sandbox.
#[test]
fn dash_env_rules() {
    let env = env_of(&[("HOME", "/h"), ("COMANDOS_DASH_URL", "")]);
    let live = parse_args(&args(&[]), true, &env).expect("vacío = omisión");
    assert_eq!(live.dash_url(), Some("http://127.0.0.1:4777"));
    let env = env_of(&[
        ("HOME", "/h"),
        ("COMANDOS_DASH_URL", "http://127.0.0.1:7311/"),
    ]);
    assert_eq!(
        parse_args(&args(&[]), true, &env).expect("live").dash_url(),
        Some("http://127.0.0.1:7311")
    );
    let shadow = parse_args(&args(&["--mode", "shadow"]), true, &env).expect("shadow");
    assert_eq!(shadow.dash_url(), Some("http://127.0.0.1:7311"));
    let sandbox = parse_args(&args(&["--mode", "sandbox"]), true, &env).expect("sandbox");
    assert_eq!(sandbox.dash_url(), None);
    let env = env_of(&[("HOME", "/h"), ("COMANDOS_DASH_URL", "http://example.com")]);
    let err = parse_args(&args(&[]), true, &env).unwrap_err();
    assert!(err.contains("COMANDOS_DASH_URL"), "{err}");
}

#[test]
fn missing_home_and_dash_led_socket_are_errors() {
    let env = env_of(&[("XDG_RUNTIME_DIR", "/run/user/1000")]);
    assert!(
        parse_args(&args(&[]), false, &env)
            .unwrap_err()
            .contains("HOME")
    );
    let env = env_of(&[("HOME", ""), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
    assert!(parse_args(&args(&[]), true, &env).is_err());
    let env = env_of(SBX_ENV);
    assert!(parse_args(&args(&["--tmux-socket", "-x"]), false, &env).is_err());
}

/// F21: `CC_LANG` de `cc-notify.conf`; si no, `$LANG`.
#[test]
fn ui_lang_reads_conf_then_lang() {
    let dir = scratch("lang");
    assert_eq!(ui_lang(&dir, Some("es_MX.UTF-8")), "es");
    assert_eq!(ui_lang(&dir, Some("en_US.UTF-8")), "en");
    assert_eq!(ui_lang(&dir, None), "en");
    std::fs::write(dir.join("cc-notify.conf"), "X=1\nCC_LANG=\"en\"\n").expect("conf");
    assert_eq!(ui_lang(&dir, Some("es_ES.UTF-8")), "en");
    std::fs::write(dir.join("cc-notify.conf"), "CC_LANG='es'\n").expect("conf");
    assert_eq!(ui_lang(&dir, Some("C")), "es");
    std::fs::write(dir.join("cc-notify.conf"), "CC_LANG=auto\n").expect("conf");
    assert_eq!(ui_lang(&dir, Some("ES_es")), "es");
    // Reglas de `read_conf` de cc-dash: la última asignación gana, `#` comenta,
    // espacios alrededor de `=` se ignoran, `export` no cuenta, comentario final no se quita.
    let cases: &[(&str, Option<&str>, &str)] = &[
        ("CC_LANG=es\nCC_LANG=auto\n", Some("en_US"), "en"),
        ("CC_LANG=en\n# CC_LANG=es\n", Some("es_MX"), "en"),
        ("  CC_LANG = 'es'  \n", None, "es"),
        ("export CC_LANG=es\n", Some("C"), "en"),
        ("CC_LANG=es # nota\n", Some("C"), "en"),
        ("CC_LANG=\"es'\n", Some("C"), "en"),
        // `\r` suelto acaba línea (modo texto universal de Python, `providers::read_conf`).
        ("CC_LANG=en\rX=1\n", Some("es_MX"), "en"),
    ];
    for (conf, lang, want) in cases {
        std::fs::write(dir.join("cc-notify.conf"), conf).expect("conf");
        assert_eq!(ui_lang(&dir, *lang), *want, "{conf:?} {lang:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
