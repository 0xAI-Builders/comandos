//! Dominio E: rutas retiradas → 410 idéntico a GET /operator, y una guarda
//! que falla si alguna vuelve a tener llamador en el código del tablero.
mod support;

use comandos_server::dash::native::retired::{GET_PATHS, POST_PATHS};
use std::{fs, path::Path};
use support::{FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, request_body};

const BODY: &str = r#"{"error": "El chat de CommandOS se retir\u00f3; usa la barra de comandos", "code": "retired"}"#;

#[tokio::test]
async fn retired_routes_answer_operator_410() {
    let home = TestHome::new("retired");
    let front = front(&home, dead_port(), home.options()).await;
    assert_eq!(GET_PATHS.len() + POST_PATHS.len(), 37);
    for path in GET_PATHS {
        for target in [path.to_string(), format!("{path}?x=1")] {
            let wire = get(front.port, &target).await;
            assert_eq!(wire.status, 410, "{target}");
            assert_eq!(wire.header("content-type"), Some("application/json"));
            assert_eq!(wire.header("cache-control"), Some("no-store"));
            assert_eq!(wire.text(), BODY);
        }
    }
    for path in POST_PATHS {
        let wire = request_body(front.port, "POST", path, "", "{}").await;
        assert_eq!((wire.status, wire.text().as_str()), (410, BODY), "{path}");
    }
    // Lo que no es exactamente una ruta retirada sigue su camino.
    assert_eq!(
        get(front.port, "/proxy-extra").await.status,
        502,
        "reenviada al heredado (muerto)"
    );
    assert_eq!(get(front.port, "/eventsx").await.status, 502);
    assert_eq!(
        get(front.port, "/events/v2").await.status,
        200,
        "nativa del dominio B"
    );
    assert_eq!(
        request_body(front.port, "POST", "/proxy-extra", "", "{}")
            .await
            .status,
        502
    );
    front.stop().await;
}

#[tokio::test]
async fn retired_body_is_pythons_operator_body() {
    let home = TestHome::new("retired-oracle");
    let front = front(&home, dead_port(), home.options()).await;
    if let Some(py) = oracle(&home).await {
        let python = get(py.port, "/operator").await;
        let rust = get(front.port, "/usage/guard").await;
        assert_eq!(
            (python.status, python.header("content-type"), python.text()),
            (rust.status, rust.header("content-type"), rust.text())
        );
    }
    front.stop().await;
}

/// Ninguna ruta retirada tiene llamador en el tablero ni en las apps.
#[test]
fn retired_routes_have_no_live_caller() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = Vec::new();
    // dash/ (js y html), bin/, lib/ y adapters/, recorridos enteros, salvo el
    // propio cc-dash y el catálogo del chat retirado, que nombra las rutas
    // para responderlas.
    for (dir, only) in [
        ("dash", true),
        ("bin", false),
        ("lib", false),
        ("adapters", false),
    ] {
        collect_sources(&repo.join(dir), only, &mut sources);
    }
    // sw.js solo nombra /events en su lista de rutas que nunca se cachean.
    let allowed = [("sw.js", "/events")];
    let paths = GET_PATHS.iter().chain(POST_PATHS.iter());
    let mut callers = Vec::new();
    for path in paths {
        for source in &sources {
            let Ok(text) = fs::read_to_string(source) else {
                continue;
            };
            let name = source.file_name().unwrap().to_string_lossy().into_owned();
            if allowed.contains(&(name.as_str(), *path)) {
                continue;
            }
            for open in ['"', '\'', '`'] {
                for close in ['"', '\'', '`', '?', '/'] {
                    let needle = format!("{open}{path}{close}");
                    // `/events/v2` es nativa (dominio B), no un llamador de `/events`.
                    let hit = text
                        .match_indices(&needle)
                        .any(|(at, _)| !text[at + needle.len()..].starts_with("v2"));
                    if hit {
                        callers.push(format!("{name}: {path}"));
                    }
                }
            }
        }
    }
    assert!(
        callers.is_empty(),
        "rutas retiradas con llamador: {callers:?}"
    );
}

fn collect_sources(dir: &Path, only_web: bool, sources: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "__pycache__" {
                collect_sources(&path, only_web, sources);
            }
            continue;
        }
        let web = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("js" | "html")
        );
        if (only_web && !web) || name == "cc-dash" || name == "operator_catalog.py" {
            continue;
        }
        sources.push(path);
    }
}

/// Método equivocado: el Python contesta él mismo, así que se reenvía.
#[tokio::test]
async fn wrong_method_is_forwarded() {
    let home = TestHome::new("retired-method");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(
        get(front.port, "/pause").await.status,
        200,
        "GET a ruta solo POST"
    );
    assert_eq!(
        request_body(front.port, "POST", "/usage/guard", "", "{}")
            .await
            .status,
        200,
        "POST a ruta solo GET"
    );
    let seen = legacy.requests();
    assert_eq!(seen.len(), 2, "{seen:?}");
    front.stop().await;
}
