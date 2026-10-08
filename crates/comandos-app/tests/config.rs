#![allow(clippy::disallowed_methods)]
use comandos_app::config::{
    AppConfig, Entry, RunMode, TmuxServer, parse_args, resolve_entry, ui_lang,
};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
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

/// Solo para sombra y live, que no tocan el disco al parsear.
const SBX_ENV: &[(&str, &str)] = &[("HOME", "/h"), ("XDG_RUNTIME_DIR", "/run/user/1000")];

/// Fixture del sandbox en un temporal propio: `run` (0700, el `XDG_RUNTIME_DIR`),
/// `tmp` (el `TMPDIR`) y `home` (el `HOME`). Las pruebas del sandbox nunca usan el
/// `/run/user/<uid>` real porque `parse_args` crea ahí la raíz.
struct Fx {
    base: PathBuf,
    run: PathBuf,
    tmp: PathBuf,
    home: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = scratch(tag);
        let (run, tmp, home) = (base.join("run"), base.join("tmp"), base.join("home"));
        for dir in [&run, &tmp, &home] {
            std::fs::create_dir_all(dir).expect("fixture");
        }
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).expect("0700");
        Self {
            base,
            run,
            tmp,
            home,
        }
    }

    fn env_with(
        &self,
        extra: &[(&'static str, String)],
    ) -> impl Fn(&str) -> Option<String> + use<> {
        let mut pairs = vec![
            ("HOME", p(&self.home)),
            ("XDG_RUNTIME_DIR", p(&self.run)),
            ("TMPDIR", p(&self.tmp)),
        ];
        for (k, v) in extra {
            pairs.retain(|(a, _)| a != k);
            pairs.push((k, v.clone()));
        }
        env_owned(pairs)
    }

    fn env(&self) -> impl Fn(&str) -> Option<String> + use<> {
        self.env_with(&[])
    }

    /// La raíz que el sandbox debe usar: `run` resuelto + `comandos-app-sbx`.
    fn root(&self) -> PathBuf {
        std::fs::canonicalize(&self.run)
            .expect("run")
            .join("comandos-app-sbx")
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

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
    let fx = Fx::new("bare");
    let env = fx.env();
    let cfg = parse_args(&args(&[]), false, &env).expect("config");
    assert_eq!(cfg.mode(), RunMode::Sandbox);
    assert_eq!(cfg.tmux_socket(), Some("comandos-app-sbx"));
    assert_eq!(cfg.hooks_dir(), fx.root().join("hooks"));
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

/// F19: salida CPython del original congelada; replay no inicia intérpretes.
#[test]
fn lock_file_name_matches_python() {
    let env = env_of(SBX_ENV);
    let live = parse_args(&args(&["--mode", "live"]), false, &env).expect("live");
    let shadow = parse_args(&args(&["--mode", "shadow"]), false, &env).expect("shadow");
    let fx = Fx::new("lock");
    let sandbox = parse_args(&args(&[]), false, &fx.env()).expect("sandbox");
    assert_eq!(live.lock_file_name(":0/x"), "cc-app-0_x.lock");
    assert_eq!(shadow.lock_file_name(":0/x"), "sombra-app-rs-0_x.lock");
    assert_eq!(sandbox.lock_file_name(":0/x"), "comandos-app-sbx-0_x.lock");
    for display in [":0", ":1", ":0/x", "", ":", "a/b:c", "host:10.0", "//"] {
        let source = "2674f366bb01b9db42f6f64b728929b3acfe8b83:bin/cc-app:_LOCK_NAME";
        let program = "import sys; d = sys.argv[1]; \
                       print(f\"cc-app-{(d or 'x').replace('/', '_').replace(':', '')}.lock\")";
        let bytes = comandos_oracle::oracle_at(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "app-lock-name",
            &serde_json::json!({"source":source,"program":program,"display":display}),
            || {
                let out = Command::new(
                    std::env::var_os("COMANDOS_APP_ORACLE_PYTHON")
                        .unwrap_or_else(|| "python3".into()),
                )
                .args(["-c", program, display])
                .env("PYTHONIOENCODING", "utf-8")
                .output()
                .map_err(|e| e.to_string())?;
                if !out.status.success() {
                    return Err(format!("Python falló con {display:?}: {}", out.status));
                }
                Ok(out.stdout)
            },
        );
        let python = String::from_utf8(bytes).expect("utf8");
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
    let fx = Fx::new("band");
    let env = fx.env();
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
    let fx = Fx::new("socket");
    let env = fx.env();
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
    let fx = Fx::new("realhooks");
    let env = fx.env();
    let hooks = fx.home.join(".claude/hooks");
    for bad in [
        hooks.clone(),
        hooks.join("sub"),
        fx.home.join(".claude/./hooks"),
    ] {
        assert!(
            parse_args(&args(&["--hooks-dir", &p(&bad)]), false, &env).is_err(),
            "{}",
            bad.display()
        );
    }
    let cfg = parse_args(
        &args(&["--hooks-dir", &p(&fx.tmp.join("sbx-hooks"))]),
        false,
        &env,
    )
    .expect("temporal");
    let tmp = std::fs::canonicalize(&fx.tmp).expect("tmp");
    assert_eq!(cfg.hooks_dir(), tmp.join("sbx-hooks"));
    assert_eq!(cfg.sandbox_root(), Some(fx.root().as_path()));
    assert_eq!(cfg.sandbox_temp(), Some(tmp.as_path()));
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
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).expect("0700");
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
    assert!(mode("live").writes_allowed());
    let fx = Fx::new("writes");
    let sandbox = parse_args(&args(&["--mode", "sandbox"]), false, &fx.env()).expect("sbx");
    assert!(sandbox.writes_allowed());
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
    let fx = Fx::new("dashenv");
    let env_sbx = fx.env_with(&[("COMANDOS_DASH_URL", "http://127.0.0.1:7311/".into())]);
    let sandbox = parse_args(&args(&["--mode", "sandbox"]), true, &env_sbx).expect("sandbox");
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

fn mode_of(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .expect("lstat")
        .permissions()
        .mode()
        & 0o7777
}

/// Ronda 2, N1: la raíz del sandbox es un directorio real y propio, creado 0700.
#[test]
fn sandbox_root_must_be_a_private_real_dir() {
    // Se crea con 0700 si no existe.
    let fx = Fx::new("rootnew");
    let cfg = parse_args(&args(&[]), false, &fx.env()).expect("nueva");
    assert_eq!(cfg.sandbox_root(), Some(fx.root().as_path()));
    assert_eq!(mode_of(&fx.root()), 0o700);
    // TMPDIR válido: no hace falta el temporal privado.
    assert!(!fx.root().join("tmp").exists());

    // La raíz como enlace: a estado real (fixture) o a cualquier otro sitio.
    let fx = Fx::new("rootlink");
    let state = fx.home.join(".claude/hooks");
    std::fs::create_dir_all(&state).expect("fixture");
    let other = fx.base.join("otro");
    std::fs::create_dir_all(&other).expect("fixture");
    for target in [&state, &other] {
        let _ = std::fs::remove_file(fx.run.join("comandos-app-sbx"));
        symlink(target, fx.run.join("comandos-app-sbx")).expect("link");
        for extra in [vec![], vec!["--hooks-dir".to_string(), p(&state)]] {
            let err = parse_args(&extra, false, &fx.env()).expect_err("enlace aceptado");
            assert!(err.contains("enlace"), "{err}");
        }
    }
    assert_eq!(std::fs::read_dir(&state).expect("hooks").count(), 0);

    // Raíz existente con escritura de grupo u otros.
    for mode in [0o770, 0o775, 0o757, 0o1777] {
        let fx = Fx::new(&format!("rootmode{mode:o}"));
        let root = fx.run.join("comandos-app-sbx");
        std::fs::create_dir(&root).expect("raíz");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(mode)).expect("chmod");
        let err = parse_args(&args(&[]), false, &fx.env()).expect_err("modo aceptado");
        assert!(
            err.contains("escritura de grupo u otros"),
            "{mode:o}: {err}"
        );
    }
    // Raíz que no es directorio.
    let fx = Fx::new("rootfile");
    std::fs::write(fx.run.join("comandos-app-sbx"), "").expect("archivo");
    assert!(parse_args(&args(&[]), false, &fx.env()).is_err());
    // `XDG_RUNTIME_DIR` con escritura de grupo: tampoco.
    let fx = Fx::new("runmode");
    std::fs::set_permissions(&fx.run, std::fs::Permissions::from_mode(0o770)).expect("chmod");
    assert!(parse_args(&args(&[]), false, &fx.env()).is_err());
    assert!(!fx.run.join("comandos-app-sbx").exists());
}

/// Ronda 2, N1/N2: ninguna ancla puede pisar estado real (fixture con `HOME` propio).
#[test]
fn sandbox_anchors_never_land_on_real_state() {
    let fx = Fx::new("anchors");
    for dir in [".claude/hooks", ".local/state", ".ssh", ".codex", "tmp"] {
        std::fs::create_dir_all(fx.home.join(dir)).expect("fixture");
    }
    symlink(fx.home.join(".claude"), fx.base.join("link-claude")).expect("link");
    let acct = fx.base.join("acct");
    std::fs::create_dir_all(acct.join("x")).expect("acct");
    let root = fx.root();
    // `XDG_RUNTIME_DIR` dentro de estado real: error, y no se crea nada ahí.
    for runtime in [
        fx.home.join(".claude"),
        fx.base.join("link-claude"),
        fx.home.join(".ssh"),
    ] {
        let env = fx.env_with(&[("XDG_RUNTIME_DIR", p(&runtime))]);
        let err = parse_args(&args(&[]), false, &env).expect_err("runtime en estado real");
        assert!(err.contains("estado real"), "{err}");
        assert!(!runtime.join("comandos-app-sbx").exists());
    }
    // TMPDIR sobre estado real (igual, dentro, contiene, enlace, CLAUDE_CONFIG_DIR, casa):
    // se descarta y el temporal pasa a `raíz/tmp`, privado.
    let tmpdirs = [
        fx.home.join(".claude"),
        fx.home.join(".claude/hooks"),
        fx.home.join(".claude/hooks/no-existe"),
        fx.home.join(".local"),
        fx.home.join(".codex"),
        fx.base.join("link-claude"),
        fx.home.clone(),
        fx.base.clone(),
        acct.join("x"),
        PathBuf::from("/"),
    ];
    for tmpdir in &tmpdirs {
        let env = fx.env_with(&[("TMPDIR", p(tmpdir)), ("CLAUDE_CONFIG_DIR", p(&acct))]);
        let cfg = parse_args(&args(&[]), false, &env)
            .unwrap_or_else(|e| panic!("{}: {e}", tmpdir.display()));
        assert_eq!(
            cfg.sandbox_temp(),
            Some(root.join("tmp").as_path()),
            "{}",
            tmpdir.display()
        );
        assert_eq!(mode_of(&root.join("tmp")), 0o700);
        for hooks in [
            fx.home.join(".claude/hooks"),
            tmpdir.join("hooks"),
            acct.join("x/h"),
        ] {
            assert!(
                parse_args(&args(&["--hooks-dir", &p(&hooks)]), false, &env).is_err(),
                "TMPDIR={} --hooks-dir {}",
                tmpdir.display(),
                hooks.display()
            );
        }
    }
    // Dentro de la casa pero fuera del estado (`~/tmp`) sigue valiendo.
    let env = fx.env_with(&[("TMPDIR", p(&fx.home.join("tmp")))]);
    let cfg = parse_args(&args(&[]), false, &env).expect("~/tmp");
    let home_tmp = std::fs::canonicalize(fx.home.join("tmp")).expect("tmp");
    assert_eq!(cfg.sandbox_temp(), Some(home_tmp.as_path()));
    assert_eq!(
        std::fs::read_dir(fx.home.join(".claude/hooks"))
            .expect("hooks")
            .count(),
        0
    );
}

/// Ronda 3, M-b: un `CLAUDE_CONFIG_DIR` con `..` tras un componente inexistente no se
/// pierde: se pliega antes de resolver, así que un `TMPDIR` igual (o dentro) se rechaza.
#[test]
fn claude_config_dir_with_dots_still_fences() {
    let fx = Fx::new("ccdots");
    let acct = fx.base.join("acct2");
    let raw = fx.base.join("no-existe/../acct2");
    let root = fx.root();
    for exists in [false, true] {
        if exists {
            std::fs::create_dir_all(acct.join("x")).expect("acct");
        }
        for tmpdir in [acct.clone(), acct.join("x"), fx.base.clone()] {
            let env = fx.env_with(&[("TMPDIR", p(&tmpdir)), ("CLAUDE_CONFIG_DIR", p(&raw))]);
            let cfg = parse_args(&args(&[]), false, &env)
                .unwrap_or_else(|e| panic!("{}: {e}", tmpdir.display()));
            assert_eq!(
                cfg.sandbox_temp(),
                Some(root.join("tmp").as_path()),
                "existe={exists} TMPDIR={}",
                tmpdir.display()
            );
        }
    }
}

/// Ronda 2: con `HOME` falso, la lista de respaldo también usa la casa de passwd. Solo
/// se pasan rutas como texto (TMPDIR y --hooks-dir); `parse_args` no escribe en ellas.
#[test]
fn faked_home_still_fences_the_passwd_home() {
    let user = nix::unistd::User::from_uid(nix::unistd::getuid())
        .expect("passwd")
        .expect("usuario");
    let fx = Fx::new("fakehome");
    let root = fx.root();
    let parent = user
        .dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    for tmpdir in [
        user.dir.join(".claude"),
        user.dir.join(".ssh"),
        user.dir.clone(),
        parent,
    ] {
        let env = fx.env_with(&[("TMPDIR", p(&tmpdir))]);
        let cfg = parse_args(&args(&[]), false, &env)
            .unwrap_or_else(|e| panic!("{}: {e}", tmpdir.display()));
        assert_eq!(
            cfg.sandbox_temp(),
            Some(root.join("tmp").as_path()),
            "{}",
            tmpdir.display()
        );
        let real_hooks = user.dir.join(".claude/hooks");
        assert!(parse_args(&args(&["--hooks-dir", &p(&real_hooks)]), false, &env).is_err());
    }
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
