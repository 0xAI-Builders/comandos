//! `launch_command`, `dialogs` y `extension_launch` contra las funciones del
//! `bin/cc-dash` D8 y de `lib/extension_launch.py` (oráculo vivo de esta rama).
//!
//! Confinamiento: cada caso usa un HOME temporal con un `PATH` que solo tiene
//! binarios falsos (`/bin/true` enlazado como `claude`, `codex`, `grok`…) y
//! `python3.11` (el lector TOML del Python 3.10). Ningún tmux, ningún agente
//! real, ninguna credencial; el gateway es un `TcpListener` propio en
//! `127.0.0.1:0`. Los procesos de `verify_launch` son `sleep` lanzados aquí.
#[path = "support/mod.rs"]
mod support;

use comandos_runtime::{
    dialogs::{self, DialogCache},
    extension_launch::{self, LaunchError},
    launch_command::{self, ConfigError, Ctx},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command},
};
use support::{Home, Oracle, registry, sha256_hex};

fn ctx(home: &Home, port: u16) -> Ctx {
    Ctx {
        registry: registry(),
        home: home.path().to_path_buf(),
        cwd: support::repo(),
        search_path: Some(home.bin().into_os_string()),
        proxy_port: port,
        repo_root: support::repo(),
    }
}

/// `{"ok": v}` / `{"error": texto, "value": true}` como lo escribe el oráculo.
fn as_python(result: &Result<String, ConfigError>) -> Value {
    match result {
        Ok(s) => json!({"ok": s}),
        Err(ConfigError::Value(m)) => json!({"error": m, "value": true}),
        Err(ConfigError::Unsure) => json!({"unsure": true}),
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn account_switch_main_has_no_config_dir() {
    let home = Home::new("cfg-main");
    let ctx = ctx(&home, free_port());
    let call = |alias: &str| {
        launch_command::configuration_command(
            &ctx,
            "claude",
            "claude",
            "",
            "",
            alias,
            "sid-1",
            &[],
            true,
        )
    };
    let to_relotto = call("relotto").unwrap();
    let to_main = call("main").unwrap();
    assert!(to_relotto.contains("CLAUDE_CONFIG_DIR=") && to_relotto.contains("relotto"));
    assert!(!to_main.contains("CLAUDE_CONFIG_DIR="));
    assert!(to_main.contains("-u CLAUDE_CONFIG_DIR"));
    let Some(oracle) = Oracle::new(&home) else {
        return;
    };
    let theirs = oracle.run(
        &[
            json!({"expr": "dash._configuration_command('claude','claude','','','relotto','sid-1',(),preserve_model_flags=True)"}),
            json!({"expr": "dash._configuration_command('claude','claude','','','main','sid-1',(),preserve_model_flags=True)"}),
        ],
        &[],
    );
    assert_eq!(
        theirs,
        vec![as_python(&Ok(to_relotto)), as_python(&Ok(to_main))]
    );
}

#[test]
fn configuration_command_matches_python() {
    let home = Home::new("cfg-cases");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let up = listener.local_addr().unwrap().port();
    let down = free_port();
    let flags: Vec<String> = [
        "--model",
        "x",
        "--effort=high",
        "-c",
        "model=\"y\"",
        "-c",
        "sandbox=\"ro\"",
        "--config=model_reasoning_effort=low",
        "--config= model =z",
        "--yolo",
        "-m",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let py_flags = serde_json::to_string(&flags).unwrap();
    // (puerto, harness, motor, modelo, esfuerzo, cuenta, reanudación, flags, preservar)
    type Case<'a> = (
        u16,
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        bool,
        bool,
    );
    let cases: Vec<Case> = vec![
        (
            up, "codex", "codex", "gpt-5.5", "high", "main", "sid", true, false,
        ),
        (
            up, "codex", "codex", "gpt-5.5", "high", "otra", "sid", true, true,
        ),
        (up, "acp", "codex", "", "", "x", "s", false, false),
        (up, "acp", "claude", "m", "", "main", "", false, false),
        (
            up,
            "opencode",
            "opencode",
            "opencode/big-pickle",
            "",
            "main",
            "ses",
            false,
            false,
        ),
        (up, "agy", "agy", "", "high", "main", "conv", false, false),
        (
            up, "grok", "grok", "grok-4.6", "max", "rel", "", false, false,
        ),
        (up, "gemini", "gemini", "", "", "main", "", false, false),
        (
            down, "claude", "codex", "gpt-5.5", "", "main", "s", false, false,
        ),
        (
            up, "claude", "codex", "gpt-5.5", "xhigh", "relotto", "s", true, false,
        ),
        (up, "claude", "grok", "", "", "main", "", false, false),
        (
            up,
            "claude",
            "claude",
            "",
            "",
            "mal/alias",
            "",
            false,
            false,
        ),
        (
            up,
            "claude",
            "claude",
            "claude-opus-5-5",
            "max",
            "",
            "x y",
            false,
            false,
        ),
        // Hostiles: comillas, `$(…)`, saltos de línea y `"` en el modelo de Codex.
        (
            up,
            "claude",
            "claude",
            "",
            "",
            "main",
            "a'b\"c $(id)\nx",
            false,
            false,
        ),
        (
            up, "codex", "codex", "gpt\"5", "hi\"gh", "main", "s;rm", false, false,
        ),
        (
            up, "grok", "grok", "m`x`", "", "main", "--resume", false, false,
        ),
        (
            up,
            "opencode",
            "opencode",
            "",
            "",
            "main",
            "ses\tión",
            false,
            false,
        ),
    ];
    let ctx_up = ctx(&home, up);
    let ctx_down = ctx(&home, down);
    let mut ours = Vec::new();
    let mut calls = Vec::new();
    for (port, harness, motor, model, effort, account, resume, with_flags, preserve) in &cases {
        let ctx = if *port == up { &ctx_up } else { &ctx_down };
        let list: &[String] = if *with_flags { &flags } else { &[] };
        ours.push(as_python(&launch_command::configuration_command(
            ctx, harness, motor, model, effort, account, resume, list, *preserve,
        )));
        calls.push(json!({
            "port": port,
            "expr": format!(
                "dash._configuration_command({harness:?},{motor:?},{model:?},{effort:?},{account:?},{resume:?},{},preserve_model_flags={})",
                if *with_flags { py_flags.as_str() } else { "()" },
                if *preserve { "True" } else { "False" },
            ),
        }));
    }
    assert!(ours.iter().all(|o| o.get("unsure").is_none()), "{ours:#?}");
    let Some(oracle) = Oracle::new(&home) else {
        return;
    };
    let theirs = oracle.run(&calls, &[]);
    for (index, (a, b)) in ours.iter().zip(&theirs).enumerate() {
        assert_eq!(a, b, "caso {index}: {:?}", cases.get(index));
    }
    drop(listener);
}

#[test]
fn model_ids_match_python() {
    let home = Home::new("cfg-models");
    let reg = registry();
    let launch = [
        ("claude", "opus-5-5"),
        ("claude", "claude-opus-5-5"),
        ("claude", "sonnet-9-9"),
        ("claude", "Haiku"),
        ("claude", "opusx-1"),
        ("codex", "gpt-5.5"),
        ("codex", "gpt-9"),
        ("claude", ""),
        ("grok", "grok-4.6"),
    ];
    let same = [
        ("claude", "claude-opus-5-5", "opus-5-5"),
        ("claude", "claude-opus-5-5", "claude-opus-5-5-20260901"),
        ("claude", "claude-opus-5-5", "claude-sonnet-5-5"),
        ("claude", "", "x"),
        ("claude", "x", ""),
        ("codex", "gpt-5.5", "GPT-5.5"),
        ("codex", "gpt-7-new", "gpt-7-new[1m]"),
    ];
    let mut ours = Vec::new();
    let mut calls = Vec::new();
    for (motor, model) in launch {
        ours.push(json!({"ok": launch_command::launch_model_id(&reg, motor, model).unwrap()}));
        calls.push(json!({"expr": format!("dash._launch_model_id({motor:?},{model:?})")}));
    }
    for (motor, expected, observed) in same {
        ours.push(
            json!({"ok": launch_command::same_model(&reg, motor, expected, observed).unwrap()}),
        );
        calls.push(
            json!({"expr": format!("dash._same_model({motor:?},{expected:?},{observed:?})")}),
        );
    }
    let Some(oracle) = Oracle::new(&home) else {
        return;
    };
    assert_eq!(ours, oracle.run(&calls, &[]));
}

// ---------------------------------------------------------------- Codex

type Seed = fn(&Home, &str);

/// Escenarios de `_inherit_codex_trust`: (nombre, siembra, cwd relativo, de, a).
fn codex_cases() -> Vec<(&'static str, Seed, &'static str, &'static str, &'static str)> {
    fn trusted(h: &Home, folder: &str) -> String {
        format!(
            "model = \"gpt-5.5\"\n[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            h.path().join(folder).display()
        )
    }
    vec![
        (
            "destino-nuevo",
            |h, _| h.put(".codex/config.toml", &trusted(h, "code/p")),
            "code/p",
            "main",
            "rel",
        ),
        (
            "ancestro-crlf",
            |h, _| {
                h.put(".codex/config.toml", &trusted(h, "code"));
                h.put(".codex-accounts/rel/config.toml", "a = 1\r\nb = \"x\"\r\n");
            },
            "code/p",
            "",
            "rel",
        ),
        (
            "destino-ya-decidio",
            |h, _| {
                h.put(".codex-accounts/otra/config.toml", &trusted(h, "code/p"));
                h.put(
                    ".codex/config.toml",
                    &format!(
                        "[projects.\"{}\"]\ntrust_level = \"untrusted\"\n",
                        h.path().join("code").display()
                    ),
                );
            },
            "code/p",
            "otra",
            "main",
        ),
        (
            "origen-sin-confianza",
            |h, _| {
                h.put(
                    ".codex/config.toml",
                    "[projects.\"/\"]\ntrust_level = \"untrusted\"\n",
                )
            },
            "code/p",
            "main",
            "rel",
        ),
        (
            "destino-variado",
            |h, _| {
                h.put(".codex/config.toml", &trusted(h, "code/p"));
                h.put(
                    ".codex-accounts/rel/config.toml",
                    "# cabecera\nmodel = 'gpt' # nota\nlist = [\n  1,\n  2,\n]\nt = {a = \"é\", b = [1, 2]}\n[projects.\"/otra\"]\ntrust_level = \"untrusted\"\n[[x]]\ny = 1",
                );
            },
            "code/p",
            "main",
            "rel",
        ),
        (
            "destino-toml-1-1",
            |h, _| {
                h.put(".codex/config.toml", &trusted(h, "code/p"));
                h.put(".codex-accounts/rel/config.toml", "a = {b = 1,\n c = 2}\n");
            },
            "code/p",
            "main",
            "rel",
        ),
        (
            "destino-roto",
            |h, _| {
                h.put(".codex/config.toml", &trusted(h, "code/p"));
                h.put(".codex-accounts/rel/config.toml", "[projects\n");
            },
            "code/p",
            "main",
            "rel",
        ),
        (
            "proyectos-lista",
            |h, _| h.put(".codex/config.toml", "projects = [1]\n"),
            "code/p",
            "main",
            "rel",
        ),
        (
            "destino-enlace",
            |h, _| {
                h.put(".codex/config.toml", &trusted(h, "code/p"));
                h.put("otro.toml", "");
                fs::create_dir_all(h.path().join(".codex-accounts/rel")).unwrap();
                std::os::unix::fs::symlink(
                    h.path().join("otro.toml"),
                    h.path().join(".codex-accounts/rel/config.toml"),
                )
                .unwrap();
            },
            "code/p",
            "main",
            "rel",
        ),
        (
            "misma-cuenta",
            |h, _| h.put(".codex/config.toml", &trusted(h, "code/p")),
            "code/p",
            "",
            "main",
        ),
    ]
}

#[test]
fn codex_trust_inheritance_matches_python() {
    for (name, seed, cwd, from, to) in codex_cases() {
        let (ours, theirs) = (Home::new("codex-a"), Home::new("codex-b"));
        for h in [&ours, &theirs] {
            fs::create_dir_all(h.path().join(cwd)).unwrap();
            seed(h, cwd);
        }
        let at = |h: &Home| h.path().join(cwd).display().to_string();
        let got = launch_command::inherit_trust_for_switch(
            &registry(),
            &ours.s(),
            &at(&ours),
            from,
            to,
            "codex",
        )
        .unwrap();
        let Some(oracle) = Oracle::new(&theirs) else {
            return;
        };
        let call = format!(
            "dash.inherit_trust_for_switch({:?},{from:?},{to:?},harness='codex')",
            at(&theirs)
        );
        let py = oracle.run(&[json!({"expr": call})], &[]);
        assert_eq!(vec![json!({"ok": got})], py, "{name}");
        // El modo de un `config.toml` que ya existía es la desviación que
        // prueba `codex_trust_keeps_the_original_mode`; aquí, el contenido.
        let unmoded = |h: &Home| {
            h.tree()
                .into_iter()
                .map(|(p, t, m)| {
                    if p.ends_with("config.toml") {
                        (p, t, 0)
                    } else {
                        (p, t, m)
                    }
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(unmoded(&ours), unmoded(&theirs), "{name}");
    }
}

/// Desviación deliberada: un `config.toml` destino que ya existía conserva su
/// modo (el Python lo dejaba con el 0600 de `mkstemp`); uno nuevo nace 0600.
#[test]
fn codex_trust_keeps_the_original_mode() {
    let h = Home::new("codex-mode");
    let cwd = h.path().join("code/p");
    fs::create_dir_all(&cwd).unwrap();
    h.put(
        ".codex/config.toml",
        &format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            cwd.display()
        ),
    );
    h.put(".codex-accounts/rel/config.toml", "a = 1\n");
    let target = h.path().join(".codex-accounts/rel/config.toml");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let cwd = cwd.display().to_string();
    let got =
        launch_command::inherit_trust_for_switch(&registry(), &h.s(), &cwd, "", "rel", "codex");
    assert_eq!(got, Ok(true));
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&target), 0o640);
    let text = fs::read_to_string(&target).unwrap();
    assert!(text.ends_with(&format!(
        "\n[projects.\"{cwd}\"]\ntrust_level = \"trusted\"\n"
    )));
    assert_eq!(
        launch_command::inherit_trust_for_switch(&registry(), &h.s(), &cwd, "", "nueva", "codex"),
        Ok(true)
    );
    assert_eq!(
        mode(&h.path().join(".codex-accounts/nueva/config.toml")),
        0o600
    );
    // Ningún temporal `.trust-*` se queda.
    let leftovers: Vec<_> = fs::read_dir(h.path().join(".codex-accounts/rel"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".trust"))
        .collect();
    assert!(leftovers.is_empty());
}

// ---------------------------------------------------------------- diálogos

#[test]
fn dialogs_match_python() {
    let home = Home::new("dialogs");
    let real = fs::read_to_string(support::repo().join("config/detectors.json")).unwrap();
    // (repo falso, contenido de config/detectors.json o None)
    let variants: Vec<(&str, Option<String>)> = vec![
        ("real", Some(real)),
        ("sin", None),
        (
            "editado",
            Some(
                json!({
                    "dialogPatterns": {
                        "trust": "Confía(s)? aquí",
                        "login": ["(", 7, "  "],
                        "onboarding": [],
                        "error": ["(^|\\n)\\s*panic:"],
                        "otro": 3
                    },
                    "verifyAttemptsWithMcp": "7"
                })
                .to_string(),
            ),
        ),
    ];
    let screens = [
        "Do you trust the files in this folder?",
        "│ Accessing workspace:\n/x",
        "  > sign in required\n",
        "note: authentication required for the admin api",
        "- Login required for admin API calls",
        "Por favor, inicia sesión para continuar",
        "Elige tu tema\n❯ Oscuro",
        " ❯ Keep xhigh\n",
        "estimated cost of max reasoning",
        "build ok\n  error: no such file",
        "x\nfatal: refusing",
        "panic: boom",
        "confías aquí",
        "─────╮\n│ ❯ hola                 │\n╰─────",
        "Παρακαλώ συνδεθείτε\n /login για συνέχεια",
        "请先登录\n/login 继续使用",
        "Войдите\n  /login чтобы продолжить",
        "Стоимость: estimated cost of max — дорого",
        "/loginλ ok",
        "/login中",
        "/login\u{301} x",
        "/login\u{2082}",
        "/login\u{200d}",
        "/login\u{203f}",
        "/login\u{24b6}",
        "estimated cost of high\u{301}",
        "Ⓐ Accessing workspace ①",
        "/login\u{31350}",
        "/login\u{870} x",
        "/login\u{1E4D0}",
        "estimated cost of max\u{1E030}",
        "",
    ];
    let mut calls = Vec::new();
    let mut ours = Vec::new();
    for (name, content) in &variants {
        let root = home.path().join("repos").join(name);
        fs::create_dir_all(root.join("config")).unwrap();
        if let Some(content) = content {
            fs::write(root.join("config/detectors.json"), content).unwrap();
        }
        let cache = DialogCache::new(&root);
        for screen in screens {
            let got = cache.screen_dialog(screen);
            assert!(got.is_ok(), "{name}: {screen:?}");
            ours.push(json!({"ok": got.unwrap()}));
            calls.push(json!({"repo": root, "expr": format!("dash.screen_dialog({})", serde_json::to_string(screen).unwrap())}));
        }
        // verify_attempts: sin MCP, con `.mcp.json` en la carpeta y con
        // `mcpServers` en el `.claude.json` de la cuenta.
        let plain = home.path().join("plain");
        let with_mcp = home.path().join("with-mcp");
        let account = home.path().join("acct");
        for dir in [&plain, &with_mcp, &account] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(with_mcp.join(".mcp.json"), "{}").unwrap();
        fs::write(
            account.join(".claude.json"),
            "{\"mcpServers\": {\"a\": {}}}",
        )
        .unwrap();
        for (cwd, dir) in [(&plain, None), (&with_mcp, None), (&plain, Some(&account))] {
            let got = dialogs::verify_attempts(
                &root,
                &cwd.display().to_string(),
                dir.map(|d| d.display().to_string()).as_deref(),
                &home.s(),
            )
            .unwrap();
            ours.push(json!({"ok": got}));
            let dir = dir.map_or("None".to_owned(), |d| {
                format!("{:?}", d.display().to_string())
            });
            calls.push(json!({"repo": root, "expr": format!("dash.verify_attempts({:?}, {dir})", cwd.display().to_string())}));
        }
    }
    let Some(oracle) = Oracle::new(&home) else {
        return;
    };
    let theirs = oracle.run(&calls, &[]);
    for (index, (a, b)) in ours.iter().zip(&theirs).enumerate() {
        assert_eq!(a, b, "{index}: {:?}", calls.get(index));
    }
    assert_eq!(ours.len(), theirs.len());
}

// ---------------------------------------------------------------- extensiones

/// Un lanzamiento preparado a mano como lo deja `prepare_launch`: directorio
/// 0700, artefactos y manifiesto 0600.
struct Prepared {
    bundle: Value,
}

fn private_file(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn prepare(home: &Home, tag: &str, harness: &str, args: Value, env: Value, count: u64) -> Prepared {
    let dir = home.path().join("launches").join(tag);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let settings = dir.join("settings.json");
    private_file(&settings, "{\"skillOverrides\": {}}");
    let manifest = dir.join("manifest.json");
    let bundle = json!({
        "manifest": manifest, "selection": {"mcps": {}, "skills": {}}, "operationId": format!("op-{tag}"),
        "harness": harness, "method": "native",
        "verification": {"marker": extension_launch::MARKER, "evidence": "manifest+process-configuration"},
    });
    let args = match args {
        Value::Null => json!(["--settings", settings, "--strict-mcp-config"]),
        other => other,
    };
    let data = json!({
        "version": 1, "bundle": bundle, "args": args, "env": env, "mounts": [],
        "codexSelectedSkillCount": count,
        "artifacts": {settings.display().to_string(): sha256_hex(&fs::read(&settings).unwrap())},
        "cwd": home.s(), "home": home.s(), "account": "main",
    });
    private_file(&manifest, &data.to_string());
    Prepared { bundle }
}

fn launch_value(result: Result<String, LaunchError>) -> Value {
    match result {
        Ok(s) => json!({"ok": s}),
        Err(LaunchError::Value(m)) => json!({"error": m, "value": true}),
        Err(LaunchError::Other) => json!({"other": true}),
        Err(LaunchError::Unsure) => json!({"unsure": true}),
    }
}

#[test]
fn wrap_command_matches_python() {
    let home = Home::new("wrap");
    let codex = prepare(
        &home,
        "codex",
        "codex",
        json!(["-c", "skills.config=[{\"path\"=\"/s\",\"enabled\"=true}]"]),
        json!({}),
        1,
    );
    let claude = prepare(&home, "claude", "claude", Value::Null, json!({}), 0);
    let open = prepare(
        &home,
        "open",
        "opencode",
        json!([]),
        json!({"OPENCODE_CONFIG_CONTENT": "{\"mcp\":{}}"}),
        0,
    );
    let tampered = prepare(&home, "tampered", "codex", json!([]), json!({}), 0);
    let manifest = |p: &Prepared| p.bundle["manifest"].as_str().unwrap().to_owned();
    // El artefacto cambia después de prepararse.
    let settings = Path::new(&manifest(&tampered)).with_file_name("settings.json");
    private_file(&settings, "{}");
    let loose = prepare(&home, "loose", "codex", json!([]), json!({}), 0);
    fs::set_permissions(manifest(&loose), fs::Permissions::from_mode(0o644)).unwrap();
    let mut other_bundle = codex.bundle.clone();
    other_bundle["operationId"] = json!("otra");
    let cases: Vec<(&str, &Value)> = vec![
        (
            "env -u CLAUDE_CONFIG_DIR codex resume sid -c 'skills.config=[{path=\"/o\",enabled=false}]'",
            &codex.bundle,
        ),
        ("codex -c 'skills.config=[1'", &codex.bundle),
        ("env -i codex", &codex.bundle),
        ("env -u", &codex.bundle),
        ("HOME=/tmp codex", &codex.bundle),
        ("FOO=1 BAR='a b' codex --yolo", &codex.bundle),
        ("codex ; rm x", &codex.bundle),
        ("codex 'abierto", &codex.bundle),
        ("", &codex.bundle),
        ("codex", &other_bundle),
        ("codex", &tampered.bundle),
        ("codex", &loose.bundle),
        (
            "claude --settings '{\"a\": 1}' --mcp-config x y --model m",
            &claude.bundle,
        ),
        ("claude --settings", &claude.bundle),
        ("claude --settings=/no/existe", &claude.bundle),
        ("claude --settings '[1]'", &claude.bundle),
        (
            "OPENCODE_CONFIG_CONTENT='{\"permission\":\"allow\"}' opencode",
            &open.bundle,
        ),
        ("OPENCODE_CONFIG_CONTENT='{roto' opencode", &open.bundle),
    ];
    let environ: HashMap<String, String> = support::oracle_environ(&home).into_iter().collect();
    let helper = extension_launch::helper(&support::repo());
    let mut ours = Vec::new();
    let mut calls = Vec::new();
    for (command, bundle) in &cases {
        ours.push(launch_value(extension_launch::wrap_command(
            command, bundle, &helper, &environ,
        )));
        calls.push(
            json!({"expr": format!("el.wrap_command({}, json.loads({}))",
            serde_json::to_string(command).unwrap(),
            serde_json::to_string(&bundle.to_string()).unwrap())}),
        );
    }
    let reference = json!({"path": "/x/env.json", "sha256": "ab"});
    ours.push(launch_value(extension_launch::wrap_environment(
        "env -u A opencode --session 's 1'",
        &reference,
        &helper,
    )));
    calls.push(json!({"expr": format!("el.wrap_environment('env -u A opencode --session \\'s 1\\'', json.loads({}))",
        serde_json::to_string(&reference.to_string()).unwrap())}));
    assert!(ours.iter().all(|o| o.get("unsure").is_none()), "{ours:#?}");
    let Some(oracle) = Oracle::new(&home) else {
        return;
    };
    let theirs = oracle.run(&calls, &[]);
    for (index, (a, b)) in ours.iter().zip(&theirs).enumerate() {
        assert_eq!(a, b, "{index}: {:?}", calls.get(index));
    }
}

#[test]
fn normalize_matches_python() {
    let home = Home::new("normalize");
    let inv = json!({
        "mcps": [
            {"id": "a", "name": "a", "enabled": true, "toggleable": true},
            {"id": "b", "name": "b", "enabled": false, "toggleable": false},
            {"id": "c", "name": "c", "enabled": null, "toggleable": false},
        ],
        "skills": [{"id": "skill:x", "name": "x", "enabled": true, "toggleable": true}],
    });
    let selections = [
        json!({}),
        json!({"mcps": {"a": false}}),
        json!({"mcps": {"b": true}}),
        json!({"mcps": {"b": false}, "skills": {"skill:x": false}}),
        json!({"mcps": {"z": true}}),
        json!({"mcps": {"a": 1}}),
        json!({"otros": {}}),
        json!([]),
        json!({"skills": []}),
    ];
    let mut ours = Vec::new();
    let mut calls = Vec::new();
    for selection in &selections {
        ours.push(match extension_launch::normalize(&inv, selection) {
            Ok(v) => json!({"ok": v}),
            Err(LaunchError::Value(m)) => json!({"error": m, "value": true}),
            Err(e) => json!({"unexpected": format!("{e:?}")}),
        });
        calls.push(
            json!({"expr": format!("el._normalize(json.loads({}), json.loads({}))",
            serde_json::to_string(&inv.to_string()).unwrap(),
            serde_json::to_string(&selection.to_string()).unwrap())}),
        );
    }
    let Some(oracle) = Oracle::new(&home) else {
        return;
    };
    assert_eq!(ours, oracle.run(&calls, &[]));
}

#[test]
fn opencode_environment_matches_python() {
    let (ours_home, theirs_home) = (Home::new("oc-a"), Home::new("oc-b"));
    let inv = json!({"mcps": [{"id": "fs"}]});
    let sources: Vec<Vec<(&str, &str)>> = vec![
        vec![],
        vec![
            ("OPENCODE_CONFIG_CONTENT", "{\"mcp\":{\"fs\":{}}}"),
            ("OPENCODE_PERMISSION", "\"allow\""),
            ("OTRA", "x"),
        ],
        vec![("OPENCODE_CONFIG", "/x.json")],
        vec![("OPENCODE_CONFIG_CONTENT", "{\"mcp\":{\"zz\":{}}}")],
        vec![("OPENCODE_CONFIG_CONTENT", "[]")],
        vec![("OPENCODE_PERMISSION", "1")],
        vec![("OPENCODE_CONFIG_CONTENT", "{\"plugin\":[\"p\"]}")],
        vec![("OPENCODE_CONFIG_CONTENT", "{\"mcp\":\"fs\"}")],
    ];
    for managed in [false, true] {
        for (index, source) in sources.iter().enumerate() {
            let dir = |h: &Home| h.path().join(format!("runtime-{managed}-{index}"));
            let map: HashMap<Vec<u8>, Vec<u8>> = source
                .iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                .collect();
            let got = extension_launch::capture_opencode_environment(
                &map,
                &dir(&ours_home),
                &inv,
                managed,
            );
            let ours = match &got {
                Ok(reference) => {
                    let path = reference["path"].as_str().unwrap();
                    let body = fs::read(path).unwrap();
                    assert_eq!(reference["sha256"], json!(sha256_hex(&body)));
                    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
                    json!({"ok": String::from_utf8(body).unwrap(), "mode": mode})
                }
                Err(LaunchError::Value(m)) => json!({"error": m, "value": true}),
                Err(e) => json!({"unexpected": format!("{e:?}")}),
            };
            let Some(oracle) = Oracle::new(&theirs_home) else {
                return;
            };
            let py_source: Vec<String> = source
                .iter()
                .map(|(k, v)| format!("{:?}: {:?}", format!("{k}"), v))
                .collect();
            let expr = format!(
                "(lambda r: {{'ok': open(r['path']).read(), 'mode': os.stat(r['path']).st_mode & 0o777}})(el.capture_opencode_environment({{k.encode(): v.encode() for k, v in {{{}}}.items()}}, {:?}, json.loads({}), managed={}))",
                py_source.join(", "),
                dir(&theirs_home).display().to_string(),
                serde_json::to_string(&inv.to_string()).unwrap(),
                if managed { "True" } else { "False" },
            );
            let theirs = oracle.run(&[json!({"expr": expr})], &[]);
            let theirs = theirs.into_iter().next().unwrap();
            let theirs = match theirs.get("ok") {
                Some(ok) => ok.clone(),
                None => theirs,
            };
            assert_eq!(ours, theirs, "{managed} {index}");
        }
    }
}

/// Un `sleep` con el entorno dado (no es un agente: solo un pid vivo).
struct Sleeper(Child);

impl Sleeper {
    fn spawn(env: &[(&str, String)]) -> Self {
        let mut command = Command::new("sleep");
        command.arg("30").env_clear();
        for (k, v) in env {
            command.env(k, v);
        }
        Self(command.spawn().unwrap())
    }
    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn verify_launch_matches_python() {
    let home = Home::new("verify");
    let prepared = prepare(&home, "live", "codex", json!(["-c", "x=1"]), json!({}), 0);
    let manifest = prepared.bundle["manifest"].as_str().unwrap().to_owned();
    let digest = sha256_hex(&fs::read(&manifest).unwrap());
    let receipt = Path::new(&manifest).with_file_name("receipt-1.json");
    let body = json!({"manifestSha256": digest, "argv": ["codex", "30"], "env": {"OPENCODE_X": null}, "artifacts": {}}).to_string();
    private_file(&receipt, &body);
    let receipt_hash = sha256_hex(body.as_bytes());
    let managed = |hash: &str| {
        vec![
            (extension_launch::MARKER, "op-live".to_owned()),
            (extension_launch::MANIFEST_ENV, manifest.clone()),
            (extension_launch::DIGEST_ENV, digest.clone()),
            (extension_launch::RECEIPT_ENV, receipt.display().to_string()),
            (extension_launch::RECEIPT_HASH_ENV, hash.to_owned()),
            ("HOME", home.s()),
        ]
    };
    let good = Sleeper::spawn(&managed(&receipt_hash));
    let bad = Sleeper::spawn(&managed("0000"));
    let external = Sleeper::spawn(&[("HOME", home.s())]);
    let mut ours = Vec::new();
    let mut calls = Vec::new();
    let bundle_py = serde_json::to_string(&prepared.bundle.to_string()).unwrap();
    for (child, label) in [(&good, "good"), (&bad, "bad"), (&external, "external")] {
        let pid = child.pid();
        ours.push(json!({"ok": extension_launch::verify_launch(pid, &prepared.bundle).unwrap()}));
        calls.push(json!({"expr": format!("el.verify_launch({pid}, json.loads({bundle_py}))")}));
        ours.push(json!({"ok": extension_launch::launch_from_pid(pid).unwrap()}));
        calls.push(json!({"expr": format!("el.launch_from_pid({pid})")}));
        for verified in [false, true] {
            ours.push(json!({"ok": extension_launch::configuration_status(Some(pid), verified)}));
            calls.push(json!({"expr": format!("el.configuration_status({pid}, {})", if verified { "True" } else { "False" })}));
        }
        let _ = label;
    }
    ours.push(json!({"ok": extension_launch::configuration_status(None, false)}));
    calls.push(json!({"expr": "el.configuration_status(None)"}));
    assert_eq!(
        ours.first(),
        Some(&json!({"ok": true})),
        "el bueno verifica"
    );
    let Some(oracle) = Oracle::new(&home) else {
        return;
    };
    let theirs = oracle.run(&calls, &[]);
    for (index, (a, b)) in ours.iter().zip(&theirs).enumerate() {
        assert_eq!(a, b, "{index}: {:?}", calls.get(index));
    }
}

// ---------------------------------------------------------- ronda 1 (revisión)

/// Siembra Codex común: el origen `main` acepta `code/p`.
fn trusted_main(h: &Home, extra: &str) -> String {
    let cwd = h.path().join("code/p");
    fs::create_dir_all(&cwd).unwrap();
    h.put(
        ".codex/config.toml",
        &format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n{extra}",
            cwd.display()
        ),
    );
    cwd.display().to_string()
}

/// Desviación deliberada (I1): un `config.toml` con fechas, en el origen o en
/// el destino, hereda la confianza. El Python 3.10 falla (sus fechas no pasan
/// por JSON) y el cambio de cuenta se quedaba en el diálogo de confianza. El
/// anexo es el mismo texto que escribiría el Python.
#[test]
fn codex_trust_with_dates_inherits() {
    let (ours, theirs) = (Home::new("dates-a"), Home::new("dates-b"));
    for h in [&ours, &theirs] {
        trusted_main(h, "since = 1979-05-27T07:32:00Z\n");
        h.put(".codex-accounts/rel/config.toml", "seen = 2026-10-05\n");
    }
    let cwd = ours.path().join("code/p").display().to_string();
    assert_eq!(
        launch_command::codex_trust_probe(&registry(), &ours.s(), &cwd, "main", "rel"),
        Ok(())
    );
    let got = launch_command::inherit_trust_for_switch(
        &registry(),
        &ours.s(),
        &cwd,
        "main",
        "rel",
        "codex",
    );
    assert_eq!(got, Ok(true));
    let text = fs::read_to_string(ours.path().join(".codex-accounts/rel/config.toml")).unwrap();
    assert_eq!(
        text,
        format!("seen = 2026-10-05\n\n[projects.\"{cwd}\"]\ntrust_level = \"trusted\"\n")
    );
    let Some(oracle) = Oracle::new(&theirs) else {
        return;
    };
    let theirs_cwd = theirs.path().join("code/p").display().to_string();
    let py = oracle.run(
        &[json!({"expr": format!("dash.inherit_trust_for_switch({theirs_cwd:?},'main','rel',harness='codex')")})],
        &[],
    );
    assert_eq!(py, vec![json!({"ok": false})], "el Python falla con fechas");
    // El anexo del port lo lee `tomllib` 3.11 con la clave exacta.
    let check = format!(
        "el._parse_toml(open({:?}).read().replace('seen = 2026-10-05', ''))['projects'][{cwd:?}]",
        ours.path()
            .join(".codex-accounts/rel/config.toml")
            .display()
            .to_string()
    );
    assert_eq!(
        oracle.run(&[json!({"expr": check})], &[]),
        vec![json!({"ok": {"trust_level": "trusted"}})]
    );
}

/// Desviación deliberada (M1): una carpeta con un emoji hereda la confianza
/// con la clave escapada como `\UXXXXXXXX`; el Python la escribía con
/// sustitutos (`😀`), que TOML rechaza.
#[test]
fn codex_trust_emoji_folder_inherits() {
    let h = Home::new("emoji");
    let cwd = h.path().join("code/p😀é");
    fs::create_dir_all(&cwd).unwrap();
    let cwd = cwd.display().to_string();
    h.put(
        ".codex/config.toml",
        &format!(
            "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            cwd.replace('😀', "\\U0001F600")
        ),
    );
    let got =
        launch_command::inherit_trust_for_switch(&registry(), &h.s(), &cwd, "", "rel", "codex");
    assert_eq!(got, Ok(true));
    let path = h.path().join(".codex-accounts/rel/config.toml");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("\\U0001F600\\u00e9\"]"), "{text}");
    let Some(oracle) = Oracle::new(&h) else {
        return;
    };
    let check = format!(
        "el._parse_toml(open({:?}).read())['projects'][{cwd:?}]['trust_level']",
        path.display().to_string()
    );
    assert_eq!(
        oracle.run(&[json!({"expr": check})], &[]),
        vec![json!({"ok": "trusted"})]
    );
    let py = oracle.run(
        &[json!({"expr": format!("dash._inherit_codex_trust({cwd:?},'','otra')")})],
        &[],
    );
    assert_eq!(
        py,
        vec![json!({"error": "Configuración TOML no soportada.", "value": true})]
    );
}

/// I2/I3: un número que `toml_edit` no representa es `Unsure` en la sonda
/// previa al `claim` y en la herencia (que tras los efectos cuenta como
/// `false`); un registro con rutas relativas también.
#[test]
fn codex_trust_probe_declines_what_it_cannot_read() {
    let h = Home::new("probe");
    let cwd = trusted_main(&h, "");
    let reg = registry();
    assert_eq!(
        launch_command::codex_trust_probe(&reg, &h.s(), &cwd, "main", "rel"),
        Ok(())
    );
    assert_eq!(
        launch_command::codex_trust_probe(&reg, &h.s(), &cwd, "", "main"),
        Ok(())
    );
    h.put(
        ".codex-accounts/rel/config.toml",
        "n = 0x8000000000000000\n",
    );
    assert!(launch_command::codex_trust_probe(&reg, &h.s(), &cwd, "main", "rel").is_err());
    assert!(
        launch_command::inherit_trust_for_switch(&reg, &h.s(), &cwd, "main", "rel", "codex")
            .is_err()
    );
    // Como la herencia, la sonda no lee un `config.toml` enlazado (no
    // heredaría): ni el destino roto tras el enlace la hace dudar.
    fs::remove_file(h.path().join(".codex-accounts/rel/config.toml")).unwrap();
    h.put("otro.toml", "n = 0x8000000000000000\n");
    std::os::unix::fs::symlink(
        h.path().join("otro.toml"),
        h.path().join(".codex-accounts/rel/config.toml"),
    )
    .unwrap();
    assert_eq!(
        launch_command::codex_trust_probe(&reg, &h.s(), &cwd, "main", "rel"),
        Ok(())
    );
    assert_eq!(
        launch_command::inherit_trust_for_switch(&reg, &h.s(), &cwd, "main", "rel", "codex"),
        Ok(false)
    );
    let mut relative = reg.clone();
    relative["harnesses"]["codex"]["accountsRoot"] = json!("cuentas-codex");
    assert!(launch_command::codex_trust_probe(&relative, &h.s(), &cwd, "main", "otra").is_err());
}

/// M3: un alias absoluto (el `from_alias` observado) descarta la raíz como
/// `os.path.join`.
#[test]
fn claude_absolute_alias_matches_python() {
    let (ours, theirs) = (Home::new("abs-a"), Home::new("abs-b"));
    for h in [&ours, &theirs] {
        let cwd = h.path().join("code/p");
        fs::create_dir_all(&cwd).unwrap();
        h.put(
            "fuera/.claude.json",
            &format!(
                "{{\"projects\": {{\"{}\": {{\"hasTrustDialogAccepted\": true}}}}}}",
                cwd.display()
            ),
        );
    }
    let args = |h: &Home| {
        (
            h.path().join("code/p").display().to_string(),
            h.path().join("fuera").display().to_string(),
        )
    };
    let (cwd, from) = args(&ours);
    let got = launch_command::inherit_trust_for_switch(
        &registry(),
        &ours.s(),
        &cwd,
        &from,
        "rel",
        "claude",
    )
    .unwrap();
    assert!(got);
    let Some(oracle) = Oracle::new(&theirs) else {
        return;
    };
    let (cwd, from) = args(&theirs);
    let py = oracle.run(
        &[json!({"expr": format!("dash.inherit_trust_for_switch({cwd:?},{from:?},'rel',harness='claude')")})],
        &[],
    );
    assert_eq!(py, vec![json!({"ok": got})]);
    let dest = |h: &Home| {
        fs::read_to_string(h.path().join(".claude-accounts/rel/.claude.json"))
            .unwrap()
            .replace(&h.s(), "~")
    };
    assert_eq!(dest(&ours), dest(&theirs));
}

/// I4: los caminos que necesitan `inventory`/`prepare_launch` se detectan
/// antes del `claim`.
#[test]
fn inventory_paths_are_detected_before_claim() {
    use extension_launch::{InventoryNeed, may_need_inventory, needs_inventory};
    let home = Home::new("inventory");
    let prepared = prepare(&home, "inv", "codex", json!([]), json!({}), 0);
    let manifest = prepared.bundle["manifest"].as_str().unwrap().to_owned();
    let digest = sha256_hex(&fs::read(&manifest).unwrap());
    let receipt = Path::new(&manifest).with_file_name("receipt-1.json");
    let body =
        json!({"manifestSha256": digest, "argv": [], "env": {}, "artifacts": {}}).to_string();
    private_file(&receipt, &body);
    let managed = Sleeper::spawn(&[
        (extension_launch::MARKER, "op-inv".to_owned()),
        (extension_launch::MANIFEST_ENV, manifest.clone()),
        (extension_launch::DIGEST_ENV, digest),
        (extension_launch::RECEIPT_ENV, receipt.display().to_string()),
        (
            extension_launch::RECEIPT_HASH_ENV,
            sha256_hex(body.as_bytes()),
        ),
        ("HOME", home.s()),
    ]);
    let external = Sleeper::spawn(&[("HOME", home.s())]);
    let base = InventoryNeed {
        extensions_only: false,
        from: "codex",
        to: "codex",
        same_conversation: true,
        unchanged: false,
        original_pid: Some(managed.pid()),
        return_origin_launch: None,
    };
    let need = |n: InventoryNeed| needs_inventory(&n).unwrap();
    assert!(need(base));
    assert!(!need(InventoryNeed {
        unchanged: true,
        ..base
    }));
    assert!(!need(InventoryNeed {
        to: "claude",
        ..base
    }));
    assert!(!need(InventoryNeed {
        original_pid: Some(external.pid()),
        ..base
    }));
    assert!(need(InventoryNeed {
        extensions_only: true,
        original_pid: None,
        ..base
    }));
    assert!(need(InventoryNeed {
        from: "opencode",
        original_pid: None,
        ..base
    }));
    let saved = prepared.bundle.clone();
    assert!(need(InventoryNeed {
        same_conversation: false,
        return_origin_launch: Some(&saved),
        ..base
    }));
    assert!(may_need_inventory(
        false,
        "codex",
        Some(managed.pid()),
        None
    ));
    assert!(!may_need_inventory(
        false,
        "codex",
        Some(external.pid()),
        None
    ));
    assert!(may_need_inventory(false, "opencode", None, None));
    assert!(may_need_inventory(true, "claude", None, None));
}
