//! `session_configure` (`bin/cc-dash` 3453) antes de tocar ningún pane: las
//! validaciones, lo que se declina siempre y la repetición idempotente de un
//! `requestId` ya registrado (contra el Python con el mismo journal). Las
//! operaciones completas sobre tmux están en
//! `crates/comandos-runtime/tests/session_configuration_oracle.rs`.
mod support;

use comandos_core::json::response_dumps;
use comandos_runtime::session_operations::{Journal, OperationStore, open_journal};
use comandos_server::dash::native::{Fault, Native, ops::configure::session_configure};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{TestHome, ops::clone_home};

fn answer(result: Result<(http::StatusCode, Value), Fault>) -> Value {
    match result {
        Ok((code, body)) => json!([code.as_u16(), body]),
        Err(Fault::Decline) => json!("decline"),
        Err(Fault::Error(_)) => json!("error"),
    }
}

#[tokio::test]
async fn validations_and_declines() {
    let home = TestHome::new("ops-configure-400");
    let native = Arc::new(Native::new(home.options()));
    let cases = [
        (
            json!({"session": "a b", "pane": "%1"}),
            json!([400, {"ok": false, "error": "sesión y panel exactos requeridos"}]),
        ),
        (
            json!({"session": "audit", "pane": "1"}),
            json!([400, {"ok": false, "error": "sesión y panel exactos requeridos"}]),
        ),
        (
            json!({"session": "audit", "pane": "%1", "requestId": "corto"}),
            json!([400, {"ok": false, "error": "requestId inválido"}]),
        ),
        (
            json!({"session": "audit", "pane": "%1", "requestId": "con espacio 123"}),
            json!([400, {"ok": false, "error": "requestId inválido"}]),
        ),
        // Extensions use the same native exact-pane guard before any claim.
        (
            json!({"session": "audit", "pane": "%1", "requestId": "extensiones-0001", "extensionsOnly": true}),
            json!([409, {"ok":false,"error":"el panel ya no existe","operationKey":"audit|%1"}]),
        ),
        // Un tipo que el `str()` del Python convertiría: se declina.
        (json!({"session": 5, "pane": "%1"}), json!("decline")),
    ];
    for (data, expected) in cases {
        assert_eq!(
            answer(session_configure(&native, data.clone()).await),
            expected,
            "{data}"
        );
    }
    assert!(
        !home.journal_db().exists() || {
            let conn = open_journal(&home.journal_db()).unwrap();
            let count: i64 = conn
                .query_row("SELECT count(*) FROM session_operations", [], |r| r.get(0))
                .unwrap();
            count == 0
        }
    );
}

/// Una operación ya registrada se repite sin tocar tmux (`store.get`
/// primero), y con otra configuración el `requestId` es un conflicto.
#[tokio::test]
async fn replay_and_conflict_match_python() {
    let home = TestHome::new("ops-configure-replay");
    let data = json!({"session": "audit", "pane": "%7", "requestId": "repetida-0001",
                      "accountOnly": true, "harnessAccount": "work", "interrupt": true});
    {
        let conn = open_journal(&home.journal_db()).unwrap();
        // Dueño vivo (esta prueba): `recover_abandoned` no toca la fila en curso.
        let owner = || i64::from(std::process::id());
        let clock = || 1_000.0;
        let store = OperationStore::new(&conn, &owner, &clock).unwrap();
        assert!(
            store
                .claim("repetida-0001", "sock|1|2|$0|%7|9", &data)
                .unwrap()
        );
        let result = json!({"ok": true, "observed": {"harness": "codex", "harnessAccount": "work"},
                            "continuity": "resumed", "handoffRequired": false});
        store
            .stage("repetida-0001", "confirmed", None, Some(&result))
            .unwrap();
        let pending = json!({"session": "audit", "pane": "%8", "requestId": "en-curso-0001"});
        assert!(
            store
                .claim("en-curso-0001", "sock|1|2|$0|%8|9", &pending)
                .unwrap()
        );
        store.stage("en-curso-0001", "waiting", None, None).unwrap();
    }
    let twin = clone_home(&home, "ops-configure-replay-py");
    let native = Arc::new(Native::new(home.options()));
    let other = json!({"session": "audit", "pane": "%7", "requestId": "repetida-0001",
                       "accountOnly": true, "harnessAccount": "main", "interrupt": true});
    let waiting = json!({"session": "audit", "pane": "%8", "requestId": "en-curso-0001"});
    let rust = json!([
        answer(session_configure(&native, data.clone()).await),
        answer(session_configure(&native, other.clone()).await),
        answer(session_configure(&native, waiting.clone()).await),
    ]);
    assert_eq!(rust[0][0], json!(200), "{rust}");
    assert_eq!(rust[1][0], json!(409), "{rust}");
    assert_eq!(rust[2][0], json!(202), "{rust}");
    let code = format!(
        "import json,sqlite3,os\n# Fixture: two claims at clock 1000, one confirmed and one waiting; owner is the live test process.\nconn=sqlite3.connect(os.path.join(dash.HOOKS, 'session-operations.sqlite3'))\nbefore=list(conn.iterdump())\nout = [list(dash.session_configure(json.loads(s))) for s in {:?}]\nassert list(conn.iterdump()) == before, 'cached requests must not mutate the journal'\nprint(json.dumps(out))",
        [data.to_string(), other.to_string(), waiting.to_string()]
    );
    // The native journal and its actual live owner are authoritative in replay.
    // No source journal, PID or actor is restored from the expected response.
    let expected = support::http_golden::dash_files(
        &twin,
        "server-ops-configure-cached",
        &[],
        &code,
        &Default::default(),
    );
    assert_eq!(response_dumps(&rust).unwrap(), expected.trim_end());
}
