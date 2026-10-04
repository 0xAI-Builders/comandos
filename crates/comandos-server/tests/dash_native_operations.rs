//! Dominio H: GET /model/status sobre el journal de operaciones.
mod support;

use comandos_runtime::session_operations::open_journal;
use comandos_server::{
    Request,
    dash::native::{Native, NativeRoute, Outcome},
};
use rusqlite::params;
use support::{FakeLegacy, TestHome, dead_port, front, get, oracle::oracle};

/// Un pid que no existe: por encima de cualquier `pid_max`.
const DEAD: i64 = 2_147_483_647;

type Row = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    i64,
    Option<&'static str>,
    Option<&'static str>,
    f64,
);

fn seed(home: &TestHome, rows: &[Row]) {
    let conn = open_journal(&home.journal_db()).unwrap();
    for (id, pane_key, request, state, owner, snapshot, result, updated) in rows {
        conn.execute(
            "INSERT INTO session_operations VALUES (?,?,?,?,?,?,?,?,?)",
            params![
                id, pane_key, "f", request, state, owner, snapshot, result, updated
            ],
        )
        .unwrap();
    }
}

fn rows(alive: i64) -> Vec<Row> {
    vec![
        (
            "op-ok",
            "s1|%1",
            r#"{"session": "s1", "pane": "%1"}"#,
            "confirmed",
            alive,
            Some(
                r#"{"origin": {"agent": "claude", "observed": {"conversationId": "c-1"}, "handoffPath": "/tmp/h"}}"#,
            ),
            Some(r#"{"ok": true, "model": "m", "observed": {"harness": "codex", "model": "gpt"}}"#),
            1791115200.25,
        ),
        (
            "op-run",
            "s2|%2",
            r#"{"session": "s2", "pane": "%2"}"#,
            "applying",
            alive,
            None,
            Some(r#"{"ok": true, "pending": true}"#),
            1791115201.5,
        ),
        (
            "op-dead",
            "s3",
            r#"{"session": "s3", "pane": ""}"#,
            "validating",
            DEAD,
            None,
            None,
            1791115202.0,
        ),
    ]
}

fn get_request(target: &str) -> Request {
    Request {
        method: http::Method::GET,
        target: target.into(),
        peer: "127.0.0.1:12345".parse().unwrap(),
        headers: vec![],
        data: None,
        body: bytes::Bytes::new(),
        internal_producer: false,
    }
}

/// `ts` lo escribe el reloj de cada proceso al recuperar: se iguala antes de comparar.
fn masked(text: &str) -> String {
    let key = "\"ts\": ";
    let Some(at) = text.find(key) else {
        return text.to_owned();
    };
    let start = at + key.len();
    let end = start
        + text[start..]
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'.')
            .count();
    format!("{}0{}", &text[..start], &text[end..])
}

/// Estado, resultado y detalle del último evento, tal como quedaron en disco.
fn stored(home: &TestHome, id: &str) -> (String, Option<String>, Option<String>) {
    let conn = rusqlite::Connection::open(home.journal_db()).unwrap();
    let (state, result) = conn
        .query_row(
            "SELECT state, result FROM session_operations WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let detail = conn
        .query_row(
            "SELECT detail FROM session_operation_events WHERE operation_id=? ORDER BY rowid DESC LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .ok();
    (state, result, detail)
}

#[tokio::test]
async fn model_status_matches_python_oracle() {
    let home = TestHome::new("ops-oracle");
    let me = i64::from(std::process::id());
    seed(&home, &rows(me));
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, dead_port(), home.options()).await;
    for (at, target) in [
        "/model/status?operationKey=s1%7C%251",
        "/model/status?operationKey=s1%7C%251&operationId=op-ok",
        "/model/status?operationKey=s2%7C%252",
        "/model/status?operationKey=s3",
        "/model/status?operationKey=s3&operationId=op-dead",
        "/model/status?operationKey=a%20b",
        "/model/status?operationKey=s1%7Cx",
        "/model/status?operationKey=s1&operationId=%24%24",
        "/model/status?operationKey=s1&operationId=op-ok",
        "/model/status?operationKey=s1&operationId=nadie",
        "/model/status?operationKey=&operationKey=s1%7C%251",
    ]
    .into_iter()
    .enumerate()
    {
        // Rust primero en la mitad de los casos: recover_abandoned es idempotente.
        let (a, b) = if at % 2 == 0 {
            let b = get(front.port, target).await;
            (get(py.port, target).await, b)
        } else {
            let a = get(py.port, target).await;
            (a, get(front.port, target).await)
        };
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    let b = get(front.port, "/model/status?operationKey=s1%7C%251").await;
    assert_eq!(
        b.text(),
        ascii(
            r#"{"operationId": "op-ok", "state": "confirmed", "ts": 1791115200.25, "ok": true, "model": "gpt", "observed": {"harness": "codex", "model": "gpt"}, "detail": "configuración confirmada", "harness": "codex", "sourceHarness": "claude", "sourceConversationId": "c-1", "handoffPath": "/tmp/h", "operationKey": "s1|%1"}"#
        )
    );
    let b = get(front.port, "/model/status?operationKey=s2%7C%252").await;
    assert_eq!(
        b.text(),
        r#"{"operationId": "op-run", "state": "applying", "ts": 1791115201.5, "pending": true, "stage": "applying", "stageCode": "applying", "sourceHarness": "", "sourceConversationId": "", "handoffPath": "", "operationKey": "s2|%2"}"#
    );
    front.stop().await;
}

/// La recuperación que hace el Rust deja en disco los mismos bytes que la del
/// Python (`json.dumps` con orden de inserción): dos HOME idénticos, uno por
/// cada proceso.
#[tokio::test]
async fn model_status_recovery_writes_python_bytes() {
    let (py_home, rust_home) = (TestHome::new("ops-rec-py"), TestHome::new("ops-rec-rs"));
    let dead: Vec<Row> = vec![
        (
            "op-val",
            "s5|%5",
            r#"{"session": "s5", "pane": "%5"}"#,
            "validating",
            DEAD,
            None,
            None,
            10.5,
        ),
        (
            "op-app",
            "s6",
            r#"{"session": "s6", "pane": ""}"#,
            "applying",
            DEAD,
            Some(r#"{"origin": {"agent": "codex"}}"#),
            None,
            11.0,
        ),
    ];
    seed(&py_home, &dead);
    seed(&rust_home, &dead);
    let Some(py) = oracle(&py_home).await else {
        return;
    };
    let front = front(&rust_home, dead_port(), rust_home.options()).await;
    for target in [
        "/model/status?operationKey=s5%7C%255",
        "/model/status?operationKey=s6&operationId=op-app",
    ] {
        let (a, b) = (get(py.port, target).await, get(front.port, target).await);
        assert_eq!(a.status, 200, "{}", a.text());
        assert_eq!((a.status, masked(&a.text())), (b.status, masked(&b.text())));
    }
    for id in ["op-val", "op-app"] {
        assert_eq!(stored(&py_home, id), stored(&rust_home, id), "{id}");
    }
    assert_eq!(
        stored(&rust_home, "op-app").1.as_deref(),
        Some(
            ascii(r#"{"ok": false, "error": "operación interrumpida; revisar recuperación", "recoveryRequired": true}"#).as_str()
        )
    );
    front.stop().await;
}

#[tokio::test]
async fn model_status_recovers_dead_owner_natively() {
    let home = TestHome::new("ops-recover");
    seed(&home, &rows(i64::from(std::process::id())));
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/model/status?operationKey=s3").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(
        wire.text().contains(r#""state": "failed""#),
        "{}",
        wire.text()
    );
    assert!(
        wire.text().contains(&ascii(
            r#""detail": "operación interrumpida; revisar recuperación""#
        )),
        "{}",
        wire.text()
    );
    front.stop().await;
}

/// El dueño de una operación en curso es el Python heredado (u otro proceso),
/// nunca el frente: mientras su pid exista en /proc, el Rust no la abandona.
#[tokio::test]
async fn model_status_never_abandons_live_foreign_owner() {
    let home = TestHome::new("ops-live");
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let owner = i64::from(child.id());
    assert_ne!(owner, i64::from(std::process::id()));
    seed(
        &home,
        &[(
            "op-ajena",
            "s7|%7",
            r#"{"session": "s7", "pane": "%7"}"#,
            "applying",
            owner,
            None,
            None,
            5.0,
        )],
    );
    let front = front(&home, dead_port(), home.options()).await;
    let target = "/model/status?operationKey=s7%7C%257";
    let wire = get(front.port, target).await;
    assert_eq!(
        wire.text(),
        r#"{"operationId": "op-ajena", "state": "applying", "ts": 5.0, "stage": "applying", "stageCode": "applying", "sourceHarness": "", "sourceConversationId": "", "handoffPath": "", "operationKey": "s7|%7"}"#
    );
    assert_eq!(stored(&home, "op-ajena").0, "applying");
    child.kill().unwrap();
    child.wait().unwrap();
    let wire = get(front.port, target).await;
    assert!(
        wire.text().contains(r#""state": "recovery_required""#),
        "{}",
        wire.text()
    );
    assert_eq!(stored(&home, "op-ajena").0, "recovery_required");
    front.stop().await;
}

#[tokio::test]
async fn model_status_declines_pending_confirmation_and_motor_result() {
    let home = TestHome::new("ops-decline");
    seed(
        &home,
        &[(
            "op-wait",
            "s4|%4",
            r#"{"session": "s4", "pane": "%4"}"#,
            "awaiting_confirmation",
            i64::from(std::process::id()),
            Some(r#"{"origin": {}}"#),
            Some("{}"),
            1.0,
        )],
    );
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for target in [
        "/model/status?operationKey=s4%7C%254",
        "/model/status?operationKey=sin-registro",
    ] {
        assert_eq!(
            get(front.port, target).await.text(),
            r#"{"legacy": true}"#,
            "{target}"
        );
    }
    assert_eq!(legacy.requests().len(), 2);
    front.stop().await;
}

/// Un dueño que `os.kill(pid, 0)` no trata como un pid (0, negativo o fuera de
/// `pid_t`) se reenvía sin recuperar nada.
#[tokio::test]
async fn model_status_declines_unusual_owner_before_recovering() {
    let home = TestHome::new("ops-owner");
    seed(
        &home,
        &[
            (
                "op-neg",
                "s8",
                r#"{"session": "s8", "pane": ""}"#,
                "validating",
                -7,
                None,
                None,
                1.0,
            ),
            (
                "op-dead",
                "s9",
                r#"{"session": "s9", "pane": ""}"#,
                "validating",
                DEAD,
                None,
                None,
                1.0,
            ),
        ],
    );
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(
        get(front.port, "/model/status?operationKey=s9")
            .await
            .text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(stored(&home, "op-dead").0, "validating", "nada se recuperó");
    front.stop().await;
}

/// Un journal con otras columnas (un Python más nuevo): una sola línea en
/// stderr, solo GET /model/status se reenvía y la base no se toca.
#[tokio::test]
async fn journal_unknown_columns_disable_only_journal_lane() {
    let home = TestHome::new("ops-newer");
    rusqlite::Connection::open(home.journal_db())
        .unwrap()
        .execute_batch("CREATE TABLE session_operations (id TEXT PRIMARY KEY, extra TEXT)")
        .unwrap();
    home.write("snippets.json", "[]");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for _ in 0..2 {
        assert_eq!(
            get(front.port, "/model/status?operationKey=s1")
                .await
                .text(),
            r#"{"legacy": true}"#
        );
    }
    assert_eq!(get(front.port, "/snippets").await.text(), "[]");
    assert_eq!(legacy.requests().len(), 2);
    front.stop().await;
    let native = Native::new(home.options());
    for _ in 0..2 {
        let outcome = native
            .dispatch(
                NativeRoute::ModelStatus,
                &get_request("/model/status?operationKey=s1"),
            )
            .await
            .unwrap();
        assert!(matches!(outcome, Outcome::Decline));
    }
    assert_eq!(native.journal_lane().refusals(), 1, "una sola línea");
    assert!(native.enabled());
    native.shutdown().await;
    let conn = rusqlite::Connection::open(home.journal_db()).unwrap();
    let objects: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    // La tabla y su índice automático de PRIMARY KEY; ni índices ni eventos nuevos.
    assert_eq!(objects, 2, "la base no se toca");
}

/// `ensure_ascii` del Python: cada carácter no ASCII como escape UTF-16.
fn ascii(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.push('\\');
                out.push_str(&format!("u{unit:04x}"));
            }
        }
    }
    out
}
