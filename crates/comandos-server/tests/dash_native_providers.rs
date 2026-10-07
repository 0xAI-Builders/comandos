//! /providers, /optimization/plans y /accounts contra el cc-dash del repo
//! (Tarea 4 de la 2e).
mod support;
use serde_json::json;
use std::path::Path;
use support::{TestHome, dead_port, front, get, http_golden::FrozenHttp};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Cuentas de Claude y Codex en el HOME temporal, sin ningún token de Claude
/// (el refresco de límites del Python nunca sale a la red), y un catálogo de
/// modelos de Grok.
fn seed_accounts(home: &TestHome) {
    let accounts = home.root.join(".claude-accounts/relotto");
    write(&accounts.join(".credentials.json"), "{}");
    // Un directorio con nombre de bandera (como el `--dangerously-skip-permissions`
    // del HOME real): no es un alias válido y ninguno de los dos lo lista.
    write(
        &home
            .root
            .join(".claude-accounts/--dangerously-skip-permissions/.credentials.json"),
        "{}",
    );
    std::fs::create_dir_all(home.root.join(".claude-accounts/relotto.lock")).unwrap();
    write(
        &home.root.join(".codex/auth.json"),
        &json!({"tokens": {"access_token": "a", "id_token": "x.e30.y"}}).to_string(),
    );
    write(
        &home.root.join(".codex-accounts/trabajo/auth.json"),
        r#"{"OPENAI_API_KEY": "k"}"#,
    );
    write(
        &home.root.join(".grok/models_cache.json"),
        &json!({"models": {
            "grok-9": {"info": {"id": "grok-9", "name": "Grok 9", "context_window": 1000,
                "reasoning_efforts": [{"id": "low"}], "reasoning_effort": "low"}},
            "grok-oculto": {"info": {"hidden": true}}
        }})
        .to_string(),
    );
    std::fs::write(
        home.hooks().join("optimization-default.json"),
        r#"{"profile": "ahorro"}"#,
    )
    .unwrap();
}

#[tokio::test]
async fn provider_routes_match_python() {
    let home = TestHome::new("providers");
    seed_accounts(&home);
    let repo = home.root.join("repo");
    std::fs::create_dir_all(repo.join("config")).unwrap();
    for name in [
        "providers.json",
        "model-tiers.json",
        "optimization-plans.json",
    ] {
        std::fs::copy(
            support::repo().join("config").join(name),
            repo.join("config").join(name),
        )
        .unwrap();
    }
    // The original's confined socket policy rejects external ports. Give both
    // implementations the same private closed-port fixture, never a product
    // proxy whose actual service state could differ from the source sandbox.
    let proxy_port = dead_port();
    std::fs::write(
        repo.join("config/proxy.json"),
        json!({"port":proxy_port}).to_string(),
    )
    .unwrap();
    let py = FrozenHttp::new_rooted_with(
        &home,
        "providers-routes",
        &[],
        "dash.PROXY_FILE = os.path.join(os.environ['HOME'], 'repo', 'config', 'proxy.json')",
    )
    .await;
    let mut opts = support::http_golden::confined_front_options(&home, &[]);
    opts.repo_root = Some(repo.clone());
    assert_eq!(
        comandos_runtime::providers::proxy_port(&repo).unwrap(),
        proxy_port
    );
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", proxy_port))
            .await
            .is_err(),
        "private proxy fixture must be down"
    );
    let front = front(&home, dead_port(), opts).await;
    for target in [
        "/providers",
        "/providers?x=1",
        "/optimization/plans",
        "/accounts",
        "/accounts?harness=codex",
        "/accounts?harness=codex&usage=0",
        "/accounts?usage=0",
        "/accounts?usage=0&usage=0",
        "/accounts?usage=1",
        "/accounts?harness=grok",
        "/accounts?harness=shell",
        "/accounts?harness=nada",
        "/accounts?harness=",
        "/accounts?harness=codex&harness=claude",
    ] {
        let _ = get(front.port, target).await;
        let a = get(front.port, target).await;
        let b = py.get(target).await;
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    // Lo que se comparó cubre de verdad las ramas.
    let accounts = get(front.port, "/accounts?usage=0").await;
    assert_eq!(accounts.status, 200);
    let body: serde_json::Value = serde_json::from_str(&accounts.text()).unwrap();
    let aliases: Vec<&str> = body["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["alias"].as_str().unwrap())
        .collect();
    assert_eq!(aliases, ["main", "relotto"]);
    let codex = get(front.port, "/accounts?harness=codex").await;
    assert!(
        codex.text().contains(r#""selectable": true"#),
        "{}",
        codex.text()
    );
    assert_eq!(get(front.port, "/accounts?harness=shell").await.status, 404);
    let providers = get(front.port, "/providers").await.text();
    assert!(providers.contains(r#""grok-9""#) && !providers.contains("grok-oculto"));
    assert!(providers.contains("midSessionRoutes") && !providers.contains("authFile"));
    let state: serde_json::Value = serde_json::from_str(&providers).unwrap();
    let route = state["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "codex:grok")
        .unwrap();
    assert_eq!(route["reason"]["code"], "gateway_down");
    let plans = get(front.port, "/optimization/plans").await.text();
    assert!(plans.contains(r#""active": "ahorro""#), "{plans}");
    front.stop().await;
}

#[tokio::test]
async fn invalid_registry_declines() {
    let home = TestHome::new("providers-bad");
    let repo = home.root.join("repo");
    write(&repo.join("config/providers.json"), "{roto");
    let legacy = support::FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(repo);
    let front = front(&home, legacy.port, opts).await;
    for target in [
        "/providers",
        "/optimization/plans",
        "/accounts",
        "/accounts?usage=0",
    ] {
        assert_eq!(
            get(front.port, target).await.text(),
            r#"{"legacy": true}"#,
            "{target}"
        );
    }
    // Las rutas vecinas siguen reenviadas: la llave es la ruta exacta.
    assert_eq!(
        get(front.port, "/accountsX").await.text(),
        r#"{"legacy": true}"#
    );
    front.stop().await;
}

#[tokio::test]
async fn accounts_decline_with_usage_lane_down() {
    // D10: con el carril de uso apagado (una base más nueva) /accounts declina;
    // /providers y /optimization/plans no leen la base y siguen nativas.
    let home = TestHome::new("providers-lane-down");
    support::seed_usage(&home, "pragma user_version = 12;");
    let legacy = support::FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    // El primer uso del carril lo apaga.
    let _ = get(front.port, "/analytics/week").await;
    assert_eq!(
        get(front.port, "/accounts").await.text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(get(front.port, "/providers").await.status, 200);
    assert!(
        !get(front.port, "/providers")
            .await
            .text()
            .contains("legacy")
    );
    front.stop().await;
}

/// `optimization_plans()` sobre catálogos raros: el Python con
/// `OPTIMIZATION_FILE` en un repo temporal y el frente con ese `repo_root`.
#[tokio::test]
async fn optimization_plans_edge_cases_match_python() {
    const PLANS: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, plans = sys.argv[1], sys.argv[2]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
dash.OPTIMIZATION_FILE = plans
try:
    print(json.dumps(dash.optimization_plans()))
except Exception as e:
    print(json.dumps({"raise": type(e).__name__}))
"#;
    let cases = [
        json!({"plans": [
            {"id": "a", "label": "A", "extra": 1, "variants": {
                "claude": {"routeId": "claude:claude", "model": "fable", "effort": "high", "selectable": "x"},
                "codex": {"routeId": "codex:codex", "model": "nada"},
                "grok": {"routeId": "grok:grok", "model": "grok-4.5", "effort": "xhigh"},
                "acp": {"routeId": "acp:claude", "model": "claude-opus-5"},
                "vacia": {}
            }},
            {"id": "b"},
            {"variants": null}
        ]}),
        json!({"plans": []}),
        json!({"otra": 1}),
        json!({"plans": "ab"}),
        json!({"plans": [{"variants": {"claude": [1]}}]}),
        json!({"plans": [{"variants": {"claude": "x"}}]}),
        json!([1]),
    ];
    for (n, case) in cases.iter().enumerate() {
        let home = TestHome::new(&format!("plans-{n}"));
        let repo = home.root.join("repo");
        std::fs::create_dir_all(repo.join("config")).unwrap();
        std::fs::copy(
            support::repo().join("config/providers.json"),
            repo.join("config/providers.json"),
        )
        .unwrap();
        let plans = repo.join("config/optimization-plans.json");
        std::fs::write(&plans, case.to_string()).unwrap();
        support::oracle::fake_effects(&home.root.join("fakebin"));
        let out = comandos_oracle::oracle_at(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "providers-plans",
            &json!({"source":support::frozen::SOURCE_COMMIT, "python":"3.10.12",
                "script":PLANS, "plans":case}),
            || {
                support::frozen::run_python_original(PLANS, &[plans.as_os_str()], &home.root)
                    .map(String::into_bytes)
            },
        );
        let out = String::from_utf8(out).unwrap();
        let legacy = support::FakeLegacy::start().await;
        let mut opts = home.options();
        opts.repo_root = Some(repo);
        opts.search_path = Some(
            format!(
                "{}:{}",
                home.root.join("fakebin").display(),
                std::env::var("PATH").unwrap_or_default()
            )
            .into(),
        );
        let front = front(&home, legacy.port, opts).await;
        let got = get(front.port, "/optimization/plans").await;
        if out.contains(r#""raise""#) {
            assert_eq!(got.text(), r#"{"legacy": true}"#, "caso {n}: {out}");
        } else {
            assert_eq!(got.status, 200, "caso {n}");
            assert_eq!(got.text(), out.trim_end(), "caso {n}");
        }
        front.stop().await;
    }
}
