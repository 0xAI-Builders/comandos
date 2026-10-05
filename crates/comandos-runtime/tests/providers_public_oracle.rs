//! Estado público del registro (`provider_public_state`), rutas a mitad de
//! sesión, `validate_selection`, `model_spec` y los modelos de Grok contra
//! `bin/cc-dash` y `lib/providers.py` de este checkout (Tarea 4 de la 2e).
#[path = "support/python.rs"]
mod python;

use comandos_core::json::response_dumps;
use comandos_runtime::model_catalog::catalog_paths;
use comandos_runtime::providers::{
    RegistryCache, complete_public_state, evaluate_capability_matrix, grok_models, model_key,
    model_spec, public_state, runtime_facts, selectable_routes, session_change_support,
    validate_selection,
};
use python::{repo, run_python};
use serde_json::{Value, json};
use std::{fs, path::Path};

/// El registro lo hidrata el propio `load_provider_registry` con el HOME
/// temporal; `which`, las cuentas, el proxy y `shutil.which` salen del caso.
/// Los modelos de Grok son los reales de `~/.grok/models_cache.json`.
const ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, case = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
c = json.load(open(case))
reg = dash.load_provider_registry()
dash.provider_registry.which = lambda b: "/bin/" + b if b in c["available"] else None
dash.account_registry.public_accounts = lambda r: c["discovered"]
dash.proxy_alive = lambda: c["alive"]
dash.shutil.which = lambda n: "/bin/" + n if c["installed"] else None
matrix = dash.capability_matrix()
selections = []
for sel, scope in c["selections"]:
    try:
        selections.append({"ok": dash.provider_registry.validate_selection(reg, matrix, sel, scope)})
    except Exception as e:
        selections.append({"err": str(e)})
print(json.dumps({
    "public": dash.provider_public_state(),
    "mid": dash.session_change_support(reg),
    "routes": sorted(x["id"] for x in matrix if x.get("selectable")),
    "selections": selections,
    "specs": [dash.provider_registry.model_spec(reg, o, m, section=s) for o, m, s in c["specs"]],
    "keys": [dash.provider_registry.model_key(k) for k in c["keys"]],
}))
"#;

fn temp_home(tag: &str) -> std::path::PathBuf {
    let home = std::env::temp_dir().join(format!("cmd-prov-pub-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    home
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Catálogo de Grok con modelos ocultos, sin `info`, esfuerzos de varias
/// formas, ventanas como texto y flotante, e ids repetidos (orden estable).
fn grok_cache() -> Value {
    json!({"models": {
        "grok-4.6": {"info": {"id": "grok-4.6", "name": "Grok 4.6", "context_window": 500000,
            "reasoning_efforts": [{"id": "low"}, {"id": "high"}, "x", {"id": ""}, {"name": "y"}],
            "reasoning_effort": "high"}},
        "grok-oculto": {"info": {"hidden": true, "context_window": 1}},
        "grok-sin-info": null,
        "grok-texto": {"info": {"context_window": " 42 ", "reasoning_efforts": "abc",
            "name": "", "reasoning_effort": 3}},
        "grok-flotante": {"info": {"id": "grok-4.6", "context_window": 1.9e3,
            "reasoning_efforts": {"low": 1}}},
        "a-primero": {}
    }})
}

fn cases() -> Vec<Value> {
    let selections = json!([
        [{"routeId": "claude:claude", "model": "claude-fable-5[1m]", "effort": "high"}, "session_model"],
        [{"routeId": "claude:claude", "model": "fable", "effort": ""}, "session_model"],
        [{"routeId": "claude:claude", "model": "nada", "effort": "high"}, "session_model"],
        [{"routeId": "claude:claude", "model": "claude-opus-5", "effort": "turbo"}, "session_model"],
        [{"routeId": "codex:codex", "model": "gpt-5.5", "effort": "low"}, "session_motor"],
        [{"routeId": "codex:codex", "model": "gpt-5.5", "effort": "low"}, "session_model"],
        [{"routeId": "codex:claude", "model": "x"}, "session_model"],
        [{"routeId": "no:existe"}, "session_model"],
        [{}, "session_model"],
        [{"routeId": "claude:grok", "model": "grok-4.5"}, "new_session"],
        [{"routeId": "grok:grok", "model": "grok", "effort": "xhigh"}, "session_model"],
    ]);
    let specs = json!([
        ["claude", "fable", "motors"],
        ["claude", "claude-fable-5-20260901", "motors"],
        ["claude", "CLAUDE-OPUS-5[1m]", "motors"],
        ["claude", "opus", "motors"],
        ["codex", "gpt", "motors"],
        ["codex", "gpt-5.6-sol[1m]", "motors"],
        ["grok", "grok", "motors"],
        ["nada", "x", "motors"],
        ["claude", "", "motors"],
        ["claude", "claude-fable-5", "harnesses"],
        ["grok", "grok-4.5", "harnesses"]
    ]);
    let keys = json!([
        "Claude-Fable-5[1m]",
        "claude-opus-5-20260901",
        "gpt-5.6-sol",
        "x-",
        "claude-",
        "",
        "a[b[c",
        "m-1234567"
    ]);
    vec![
        json!({
            "available": ["claude", "codex"],
            "discovered": {
                "claude": [
                    {"provider": "claude", "alias": "main", "identity": "a@b", "authenticated": true, "state": "ready", "selectable": true},
                    {"provider": "claude", "alias": "relotto", "identity": "", "authenticated": false, "state": "login_required", "selectable": false}
                ],
                "codex": [{"provider": "codex", "alias": "main", "selectable": false}],
                "grok": []
            },
            "alive": false, "installed": false,
            "selections": selections, "specs": specs, "keys": keys
        }),
        json!({
            "available": ["claude", "codex", "grok", "cc-acp", "opencode", "agy", "claude-agent-acp"],
            "discovered": {
                "claude": [{"alias": "otra", "selectable": true}],
                "codex": [{"alias": "main", "selectable": true, "motorSelectable": "previo"}],
                "grok": [{"alias": "main", "selectable": true}]
            },
            "alive": true, "installed": true,
            "selections": selections, "specs": specs, "keys": keys
        }),
        json!({
            "available": [],
            "discovered": {},
            "alive": true, "installed": false,
            "selections": selections, "specs": specs, "keys": keys
        }),
    ]
}

#[test]
fn public_state_matches_python_oracle() {
    let mut codes = Vec::new();
    for (n, case) in cases().into_iter().enumerate() {
        let home = temp_home(&format!("case{n}"));
        // Credencial de Codex en `defaultHome` (el `authenticated` público)
        // y el catálogo de Grok en el HOME temporal.
        write(&home.join(".codex/auth.json"), "{}");
        write(
            &home.join(".grok/models_cache.json"),
            &grok_cache().to_string(),
        );
        let file = home.join("case.json");
        fs::write(&file, case.to_string()).unwrap();
        let Some(expected) = run_python(ORACLE, &[file.as_os_str()], &home) else {
            return;
        };
        let paths = catalog_paths(&home, &repo(), None, None);
        let registry = RegistryCache::default()
            .load(&repo().join("config/providers.json"), &paths)
            .unwrap();
        let wanted: Vec<String> = case["available"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        let available = |b: &str| wanted.iter().any(|w| w == b);
        let installed = case["installed"].as_bool().unwrap();
        let alive = case["alive"].as_bool().unwrap();
        let discovered = &case["discovered"];
        let public = public_state(&registry, &available, &home).unwrap();
        let grok = || grok_models(&home.join(".grok"));
        let full =
            complete_public_state(&registry, public, discovered, &grok, installed, alive).unwrap();
        let facts =
            runtime_facts(&registry, discovered, &available, &home, installed, alive).unwrap();
        let matrix = evaluate_capability_matrix(&registry, &facts).unwrap();
        let selections: Vec<Value> = case["selections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                match validate_selection(&registry, &matrix, &pair[0], pair[1].as_str().unwrap())
                    .unwrap()
                {
                    Ok(cell) => json!({"ok": cell}),
                    Err(code) => json!({"err": code}),
                }
            })
            .collect();
        let specs: Vec<Value> = case["specs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                model_spec(
                    &registry,
                    s[0].as_str().unwrap(),
                    s[1].as_str().unwrap(),
                    s[2].as_str().unwrap(),
                )
                .unwrap()
                .unwrap_or(Value::Null)
            })
            .collect();
        let keys: Vec<String> = case["keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| model_key(k.as_str().unwrap()).unwrap())
            .collect();
        let got = json!({
            "public": full,
            "mid": session_change_support(&registry).unwrap(),
            "routes": selectable_routes(&registry, &facts).unwrap(),
            "selections": selections,
            "specs": specs,
            "keys": keys,
        });
        assert_eq!(
            response_dumps(&got).unwrap(),
            expected.trim_end(),
            "caso {n}"
        );
        // La prueba cubre de verdad las ramas: cuentas, modelos de Grok,
        // celdas `not_routed` y los códigos de error.
        codes.extend(
            got["selections"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|s| s.get("err").cloned()),
        );
        if n == 0 {
            let harnesses = &got["public"]["harnesses"];
            assert_eq!(
                harnesses["claude"]["accounts"][0]["motorSelectable"],
                json!(true)
            );
            assert_eq!(
                harnesses["claude"]["accounts"][1]["motorSelectable"],
                json!(false)
            );
            assert_eq!(harnesses["grok"]["models"][0]["id"], json!("grok-texto"));
            assert!(harnesses["claude"].get("authFile").is_none());
            assert!(
                got["public"]["matrix"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["productState"] == json!("not_routed"))
            );
        }
        let _ = fs::remove_dir_all(&home);
    }
    for code in [
        "model_unavailable",
        "effort_unavailable",
        "route_scope_not_supported",
        "harness_login_missing",
        "'NoneType' object has no attribute 'get'",
    ] {
        assert!(codes.contains(&json!(code)), "{code}: {codes:?}");
    }
}

/// `grok_state.models()` con archivos rotos o de otra forma: el `except` del
/// Python (sin modelos) o, fuera del `try`, la excepción (`Unsure`).
#[test]
fn grok_models_edge_cases() {
    const GROK_ORACLE: &str = r#"
import json, os, sys
sys.path.insert(0, os.path.join(sys.argv[1], "lib"))
import grok_state
try:
    print(json.dumps({"ok": grok_state.models()}))
except Exception as e:
    print(json.dumps({"raise": type(e).__name__}))
"#;
    let cases: Vec<&[u8]> = vec![
        b"{roto",
        b"[1, 2]",
        b"{\"models\": []}",
        b"{\"models\": [1]}",
        b"{\"models\": {\"a\": 5}}",
        b"{\"models\": {\"a\": {\"info\": [1]}}}",
        b"{\"models\": {\"a\": {\"info\": {\"context_window\": \"x\"}}}}",
        b"{\"models\": {\"a\": {\"info\": {\"reasoning_efforts\": 3}}}}",
        b"{\"models\": {\"b\": {}, \"a\": {\"info\": {\"id\": 7, \"name\": true}}}}",
        b"\xef\xbb\xbf{\"models\": {\"a\": {}}}",
        b"\xff\xfe",
    ];
    for (n, bytes) in cases.into_iter().enumerate() {
        let home = temp_home(&format!("grok{n}"));
        write(&home.join(".grok/models_cache.json"), "");
        fs::write(home.join(".grok/models_cache.json"), bytes).unwrap();
        let Some(expected) = run_python(GROK_ORACLE, &[], &home) else {
            return;
        };
        let expected: Value = serde_json::from_str(expected.trim_end()).unwrap();
        match grok_models(&home.join(".grok")) {
            Ok(models) => assert_eq!(
                response_dumps(&json!({"ok": models})).unwrap(),
                response_dumps(&expected).unwrap(),
                "caso {n}"
            ),
            Err(_) => assert!(expected.get("raise").is_some(), "caso {n}: {expected}"),
        }
        let _ = fs::remove_dir_all(&home);
    }
    // Sin archivo: sin modelos.
    let home = temp_home("grok-none");
    assert_eq!(
        grok_models(&home.join(".grok")).unwrap(),
        Vec::<Value>::new()
    );
    let _ = fs::remove_dir_all(&home);
}
