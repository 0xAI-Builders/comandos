//! Dominio F3: /model-tiers del repositorio y /sovereignty (inventario local).
mod support;

use comandos_server::dash::{native::wall_clock_ms, repo_root};
use std::{fs, sync::Arc};
use support::{FakeLegacy, TestHome, dead_port, front, get, http_golden::FrozenHttp, oracle::oracle, repo};

fn masked(text: &str, key: &str) -> String {
    let key = format!("\"{key}\": ");
    let Some(at) = text.find(&key) else {
        return text.to_owned();
    };
    let start = at + key.len();
    let end = start + text[start..].bytes().take_while(u8::is_ascii_digit).count();
    format!("{}0{}", &text[..start], &text[end..])
}

#[test]
fn repo_root_follows_dash_index_symlink() {
    let home = TestHome::new("repo-root");
    let dash = home.root.join("dash");
    fs::create_dir_all(&dash).unwrap();
    std::os::unix::fs::symlink(repo().join("dash/index.html"), dash.join("index.html")).unwrap();
    assert_eq!(repo_root(&dash, None), fs::canonicalize(repo()).ok());
    assert_eq!(repo_root(&dash, Some("/otro")), Some("/otro".into()));
    assert_eq!(repo_root(&dash, Some("")), fs::canonicalize(repo()).ok());
    assert_eq!(repo_root(&home.root.join("nada"), None), None);
}

#[tokio::test]
async fn model_tiers_serves_repo_file_like_python() {
    let home = TestHome::new("tiers");
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/model-tiers").await;
    assert_eq!(wire.status, 200);
    assert!(wire.text().starts_with('{'));
    {
        let py = FrozenHttp::new(&home, "server-http-model-tiers", &[]).await;
        for target in ["/model-tiers", "/model-tiers?x=1"] {
            let a = py.get(target).await;
            let b = get(front.port, target).await;
            assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
            assert_eq!(a.header("content-type"), b.header("content-type"));
            assert_eq!(a.header("cache-control"), b.header("cache-control"));
        }
    }
    front.stop().await;
}

#[tokio::test]
async fn model_tiers_declines_without_readable_file() {
    let home = TestHome::new("tiers-decline");
    let fake = home.root.join("repo");
    fs::create_dir_all(fake.join("config")).unwrap();
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(fake.clone());
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(front.port, "/model-tiers").await.text(),
        r#"{"legacy": true}"#,
        "ausente"
    );
    fs::write(fake.join("config/model-tiers.json"), "{roto").unwrap();
    assert_eq!(
        get(front.port, "/model-tiers").await.text(),
        r#"{"legacy": true}"#,
        "ilegible"
    );
    fs::write(fake.join("config/model-tiers.json"), b"{\"a\": \"\xff\"}").unwrap();
    assert_eq!(
        get(front.port, "/model-tiers").await.text(),
        r#"{"legacy": true}"#,
        "no UTF-8"
    );
    fs::write(fake.join("config/model-tiers.json"), "[1]").unwrap();
    assert_eq!(
        get(front.port, "/model-tiers").await.text(),
        "{}",
        "no objeto → {{}}"
    );
    fs::write(
        fake.join("config/model-tiers.json"),
        r#"{"b": 1, "a": [2]}"#,
    )
    .unwrap();
    assert_eq!(
        get(front.port, "/model-tiers").await.text(),
        r#"{"b": 1, "a": [2]}"#
    );
    assert_eq!(legacy.requests().len(), 3, "{:?}", legacy.requests());
    front.stop().await;
}

#[tokio::test]
async fn model_tiers_without_repo_root_declines() {
    let home = TestHome::new("tiers-noroot");
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = None;
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(front.port, "/model-tiers").await.text(),
        r#"{"legacy": true}"#
    );
    front.stop().await;
}

/// HOME con algo de cada fuente del inventario.
fn seed(home: &TestHome) {
    home.write("prefs.json", "{}");
    home.write("snippets.json", "[]");
    home.write("events.jsonl", "a\nb\nc");
    home.write(
        "cc-notify.conf",
        "# x\nNATIVE_NOTIFY=\"1\"\nVOLUME=40\n  # NATIVE_NOTIFY=0\n",
    );
    home.write("providers.env", "X=1");
    fs::set_permissions(
        home.hooks().join("providers.env"),
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    home.write("state/uno.json", "{}");
    home.write("state/.oculto.json", "{}");
    fs::create_dir_all(home.hooks().join("state/dir.json")).unwrap();
    fs::create_dir_all(home.root.join(".claude/projects/p")).unwrap();
    fs::create_dir_all(home.root.join(".claude/projects/.h")).unwrap();
    fs::write(home.root.join(".claude/projects/p/t.jsonl"), "").unwrap();
    fs::write(home.root.join(".claude/projects/p/.h.jsonl"), "").unwrap();
    fs::write(home.root.join(".claude/projects/.h/t.jsonl"), "").unwrap();
    fs::write(home.root.join(".claude/projects/suelto.jsonl"), "").unwrap();
    fs::create_dir_all(home.root.join(".codex/sessions/2026/10/04")).unwrap();
    fs::write(
        home.root.join(".codex/sessions/2026/10/04/rollout-1.jsonl"),
        "",
    )
    .unwrap();
    fs::write(home.root.join(".codex/sessions/2026/10/04/otro.jsonl"), "").unwrap();
    fs::create_dir_all(home.root.join(".ssh")).unwrap();
    fs::write(
        home.root.join(".ssh/config"),
        "Host a\r\n  host b\rHost *\nhost c\nHostName d\n",
    )
    .unwrap();
    let conn = comandos_store::usage::open_usage_db_at(&home.usage_db()).unwrap();
    comandos_store::usage::ensure_schema(&conn).unwrap();
    conn.execute("insert into focus_settings(key,value) values('a','1')", [])
        .unwrap();
    drop(conn);
}

#[tokio::test]
async fn sovereignty_matches_python_oracle() {
    let home = TestHome::new("sovereignty");
    seed(&home);
    let Some(py) = oracle(&home).await else {
        return;
    };
    // Después de arrancar el oráculo: con la marca presente al arrancar, el
    // Python restauraría el terminal web real (`cc-webterm` del PATH y
    // `tailscale serve`). `support::oracle` se niega a arrancar con ella.
    home.write("webterm-enabled", "");
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    let a = get(py.port, "/sovereignty").await;
    let b = get(front.port, "/sovereignty").await;
    assert_eq!(b.status, 200, "{}", b.text());
    assert_eq!(
        masked(&a.text(), "generatedAt"),
        masked(&b.text(), "generatedAt")
    );
    assert_eq!(a.header("content-type"), b.header("content-type"));
    let text = b.text();
    assert!(
        text.contains(
            r#""label": "Servidores SSH", "path": "~/.ssh/config", "kind": "conf", "count": 3}"#
        ),
        "{text}"
    );
    assert!(text.contains(r#""kind": "json", "count": 2}"#), "{text}");
    assert!(
        text.contains(
            r#""Rollouts de Codex", "path": "~/.codex/sessions/", "kind": "jsonl", "count": 1}"#
        ),
        "{text}"
    );
    assert!(
        text.contains(r#""count": 1}, {"label": "Rollouts"#),
        "{text}"
    );
    assert!(!text.contains(TOKEN_TEXT), "el token nunca sale");
    // Sin rollouts, sin ssh, sin conf: el Python omite Codex y apaga avisos.
    fs::remove_dir_all(home.root.join(".codex")).unwrap();
    fs::remove_dir_all(home.root.join(".ssh")).unwrap();
    fs::remove_file(home.hooks().join("cc-notify.conf")).unwrap();
    fs::remove_file(home.hooks().join("webterm-enabled")).unwrap();
    let a = get(py.port, "/sovereignty").await;
    let b = get(front.port, "/sovereignty").await;
    assert_eq!(
        masked(&a.text(), "generatedAt"),
        masked(&b.text(), "generatedAt")
    );
    assert!(!b.text().contains("Rollouts"));
    front.stop().await;
}

const TOKEN_TEXT: &str = support::TOKEN;

/// El Python crea y migra la base de uso en segundo plano al arrancar: se
/// espera a que termine para no comparar contra una base a medio crear.
async fn usage_db_settled(home: &TestHome) {
    for _ in 0..200 {
        let version = rusqlite::Connection::open_with_flags(
            home.usage_db(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()
        .and_then(|c| comandos_store::usage::schema_version(&c).ok());
        if version == Some(comandos_store::usage::SCHEMA_VERSION) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("el oráculo no terminó de crear la base de uso");
}

#[tokio::test]
async fn sovereignty_bare_home_matches_python() {
    let home = TestHome::new("sovereignty-bare");
    let Some(py) = oracle(&home).await else {
        return;
    };
    usage_db_settled(&home).await;
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    let a = get(py.port, "/sovereignty").await;
    let b = get(front.port, "/sovereignty").await;
    assert_eq!(b.status, 200, "{}", b.text());
    assert_eq!(
        masked(&a.text(), "generatedAt"),
        masked(&b.text(), "generatedAt")
    );
    front.stop().await;
}

#[tokio::test]
async fn sovereignty_does_not_create_usage_db() {
    let home = TestHome::new("sovereignty-nodb");
    let front = front(&home, dead_port(), home.options()).await;
    let b = get(front.port, "/sovereignty").await;
    assert_eq!(b.status, 200, "{}", b.text());
    assert!(!b.text().contains("Uso y costos"), "{}", b.text());
    assert!(
        !home.usage_db().exists(),
        "el inventario no crea la base de uso"
    );
    front.stop().await;
}

#[tokio::test]
async fn sovereignty_generated_at_is_seconds_of_clock() {
    let home = TestHome::new("sovereignty-clock");
    let front = front(&home, dead_port(), home.options()).await;
    let b = get(front.port, "/sovereignty").await;
    assert_eq!(b.status, 200, "{}", b.text());
    assert!(
        b.text()
            .ends_with(&format!(r#""generatedAt": {}}}"#, support::NOW_MS / 1000)),
        "{}",
        b.text()
    );
    front.stop().await;
}

#[tokio::test]
async fn sovereignty_declines_on_undecodable_conf() {
    let home = TestHome::new("sovereignty-bytes");
    fs::write(
        home.hooks().join("cc-notify.conf"),
        b"NATIVE_NOTIFY=1\n\xff\n",
    )
    .unwrap();
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(
        get(front.port, "/sovereignty").await.text(),
        r#"{"legacy": true}"#
    );
    front.stop().await;
}

#[tokio::test]
async fn sovereignty_declines_on_undecodable_ssh_config() {
    let home = TestHome::new("sovereignty-ssh");
    fs::create_dir_all(home.root.join(".ssh")).unwrap();
    fs::write(home.root.join(".ssh/config"), b"Host a\n\xfe\n").unwrap();
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(
        get(front.port, "/sovereignty").await.text(),
        r#"{"legacy": true}"#
    );
    front.stop().await;
}

#[tokio::test]
async fn sovereignty_declines_on_empty_usage_db_without_creating_tables() {
    let home = TestHome::new("sovereignty-empty-db");
    fs::write(home.usage_db(), "").unwrap();
    let legacy = FakeLegacy::start().await;
    // Sin efectos de uso: el refresco de límites del arranque no abre la base.
    let mut opts = home.options();
    opts.usage_effects = false;
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(front.port, "/sovereignty").await.text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(fs::metadata(home.usage_db()).unwrap().len(), 0);
    front.stop().await;
}

#[tokio::test]
async fn sovereignty_query_uses_python_static_fallback() {
    let home = TestHome::new("sovereignty-query");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let response = get(front.port, "/sovereignty?x=1").await;
    assert_eq!(response.status, 404);
    assert!(response.text().contains("File not found"));
    assert!(legacy.seen.lock().unwrap().is_empty());
    front.stop().await;
}

/// Las declinaciones de `/sovereignty` (conf o `~/.ssh/config` no UTF-8) van
/// antes del carril de uso: una base vieja no se migra para luego reenviar.
#[tokio::test]
async fn sovereignty_declines_before_migrating_older_usage_db() {
    for (tag, rel, bytes) in [
        (
            "sovereignty-old-conf",
            ".claude/hooks/cc-notify.conf",
            &b"NATIVE_NOTIFY=1\n\xff\n"[..],
        ),
        ("sovereignty-old-ssh", ".ssh/config", &b"Host a\n\xfe\n"[..]),
    ] {
        let home = TestHome::new(tag);
        let path = home.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        let conn = rusqlite::Connection::open(home.usage_db()).unwrap();
        conn.execute_batch("create table viejo(a); pragma user_version=3")
            .unwrap();
        drop(conn);
        let legacy = FakeLegacy::start().await;
        // Sin efectos de uso: el refresco de límites del arranque (Tarea 8 de la
        // 2e) abre la base para el consumo medido de Grok y Groq y la migra, como
        // el `_limits_snapshot_loop` del Python; aquí solo cuenta `/sovereignty`.
        let mut opts = home.options();
        opts.usage_effects = false;
        let front = front(&home, legacy.port, opts).await;
        assert_eq!(
            get(front.port, "/sovereignty").await.text(),
            r#"{"legacy": true}"#,
            "{tag}"
        );
        front.stop().await;
        let conn = rusqlite::Connection::open(home.usage_db()).unwrap();
        let version: i64 = conn
            .query_row("pragma user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 3, "{tag}: la base vieja no se tocó");
        let tables: i64 = conn
            .query_row("select count(*) from sqlite_master", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 1, "{tag}: sin tablas nuevas");
    }
}
