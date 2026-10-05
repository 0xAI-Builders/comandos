//! Ayudas de prueba del corte `news` (2f-4): ediciones sembradas en el
//! app-state del HOME temporal (las filas que dejaría `_store`), imágenes en
//! `$XDG_STATE_HOME/comandos/news-media` del mismo HOME y la configuración en
//! `~/.claude/hooks`. Nada de red: ninguna fuente se descarga.
#![allow(dead_code)]
use super::TestHome;
use comandos_store::state::{self, MIGRATIONS};
use std::path::PathBuf;

/// Agente ACP falso de la Tarea 3; vacío hasta entonces.
#[derive(Default)]
pub struct FakeAcp;

/// Una imagen pequeña (se lee entera) y una grande (se envía por trozos).
pub const SMALL_IMAGE: &str = "0123456789abcdef0123456789abcdef.png";
pub const LARGE_IMAGE: &str = "fedcba9876543210fedcba9876543210.jpg";
pub const EMPTY_IMAGE: &str = "00000000000000000000000000000000.gif";
pub const MISSING_IMAGE: &str = "11111111111111111111111111111111.webp";

/// Las filas: tres ediciones legibles o vacías, una futura, fuentes con y
/// sin captura (metas rotas incluidas), noticias, chat, notas y guardadas.
pub const SEED: &str = r#"
INSERT INTO news_editions (id, local_date, slot, timezone, scheduled_at_ms, status, published_at_ms, title,
  story_count, source_count, failed_source_count, cost_usd, model, notes, lead) VALUES
 ('2026-10-03@09:00','2026-10-03','09:00','America/Mexico_City',1791039600000,'published',1791039700000,'Tu edición de IA',2,3,1,0.123456789,'acp:opencode/x','["una nota"]','Lo **importante**'),
 ('2026-10-03@15:00','2026-10-03','15:00','America/Mexico_City',1791061200000,'partial',1791061300000,'Tu edición de IA',1,1,0,0.0078125,NULL,'[]',NULL),
 ('2026-10-03@21:00','2026-10-03','21:00','America/Mexico_City',1791082800000,'empty',NULL,'Tu edición de IA',0,0,2,0,NULL,'["sin novedades","ñandú"]',NULL),
 ('2030-01-01@09:00','2030-01-01','09:00','America/Mexico_City',1893510000000,'scheduled',NULL,NULL,0,0,0,0,NULL,'[]',NULL);
INSERT INTO news_jobs (edition_id, state, attempts, lease_until_ms, started_at_ms, finished_at_ms, error) VALUES
 ('2026-10-03@09:00','done',1,NULL,1791039600000,1791039700000,NULL),
 ('2026-10-03@21:00','failed',2,NULL,1791082800000,1791082900000,'sin fuentes');
INSERT INTO news_sources (id, url, original_url, title, origin, category, published_at_ms, discovered_at_ms,
  verified_at_ms, fetch_status, fetch_error, meta) VALUES
 (1,'https://openai.com/blog/x','https://openai.com/blog/x?utm_source=a','Anuncio','openai.com','oficial',1791000000000,1791030000000,1791039700000,'ok',NULL,'{"official": true, "role": "article", "heat": ""}'),
 (2,'https://news.ycombinator.com/item?id=1','https://news.ycombinator.com/item?id=1','Hilo HN','hn','hot',NULL,1791030000000,NULL,'failed','timeout','{"role": "discussion", "heat": "alto"}'),
 (3,'https://blog.example.com/a','https://blog.example.com/a','Artículo «ñ»','blog','ia',1791000000000,1791030000000,1791039700000,'ok',NULL,'{malo'),
 (4,'https://x.example/b','https://x.example/b','Otro','x','ia',NULL,1791030000000,NULL,'not_fetched',NULL,'[1, 2]');
INSERT INTO news_captures (source_id, captured_at_ms, final_url, title, byline, lang, blocks, partial) VALUES
 (1,1791039700000,'https://openai.com/blog/x','Anuncio','Equipo','en','[{"type": "h", "text": "Título"}, {"type": "p", "text": "Párrafo 1.50"}, {"type": "img", "src": "/news/media/0123456789abcdef0123456789abcdef.png", "alt": "fig"}]',0),
 (3,1791039700000,'https://blog.example.com/a',NULL,NULL,NULL,'{"no": "lista"}',1);
INSERT INTO news_stories (id, edition_id, position, story_key, category, title, summary_md, body_md, opportunity, model, meta) VALUES
 (10,'2026-10-03@09:00',0,'k1','oficial','Lanzan X','Resumen **uno**','Cuerpo uno',NULL,'acp:opencode/x','{"heat": "alto"}'),
 (11,'2026-10-03@09:00',1,'k2','bounty','Recompensa','Resumen dos','Cuerpo dos','{"reward": "1000 USD", "deadline": null}','acp:claude/y','not json'),
 (12,'2026-10-03@15:00',0,'k3','ia','Tercera','Resumen tres','Cuerpo tres',NULL,'acp:opencode/x','[]');
INSERT INTO news_story_sources (story_id, source_id) VALUES (10,2),(10,1),(10,3),(11,4),(11,2),(12,1);
INSERT INTO news_translations (source_id, lang, state, title, blocks, model, error, created_at_ms, updated_at_ms) VALUES
 (1,'es','done','Anuncio (es)','[{"type": "h", "text": "Título es"}]','acp:opencode/x',NULL,1791040000000,1791040100000),
 (3,'es','failed',NULL,'[roto',NULL,'interrumpida',1791040000000,1791040200000);
INSERT INTO news_chat (id, story_id, edition_id, role, state, text, cite, model, created_at_ms) VALUES
 (1,10,'2026-10-03@09:00','user','done','¿Qué cambia?',NULL,NULL,1791040000000),
 (2,10,'2026-10-03@09:00','assistant','done','Cambia todo','s1','acp:opencode/x',1791040000001),
 (3,11,'2026-10-03@09:00','user','done','hola',NULL,NULL,1791040000200);
INSERT INTO news_notes (id, story_id, edition_id, story_title, kind, chat_id, quote, cite, text, created_at_ms, updated_at_ms) VALUES
 (1,10,'2026-10-03@09:00','Lanzan X','libre',NULL,NULL,NULL,'Una PALABRA clave',1791040000000,1791040000000),
 (2,10,'2026-10-03@09:00','Lanzan X','chat',2,'Cambia todo','s1','Respuesta de la IA, guardada desde el chat.',1791040000500,1791040000500),
 (3,11,'2026-10-03@09:00','Recompensa','libre',NULL,'cita con palabra',NULL,'otra',1791040000600,1791040000700),
 (4,12,'2026-10-03@15:00','Árbol','libre',NULL,NULL,NULL,'árbol',1791040000800,1791040000800);
INSERT INTO news_saved (story_id, edition_id, saved_at_ms) VALUES
 (10,'2026-10-03@09:00',1791040000000),(12,'2026-10-03@15:00',1791040000900);
"#;

/// `$XDG_STATE_HOME/comandos/news-media` del HOME (el `media_dir()` de los
/// dos lados: `confined_env` y `TestHome::options` apuntan ahí).
pub fn media_dir(home: &TestHome) -> PathBuf {
    home.root.join(".local/state/comandos/news-media")
}

/// Bytes deterministas de la imagen grande (2 MiB + 7: varios trozos).
pub fn large_image() -> Vec<u8> {
    (0..(2 * 1024 * 1024 + 7))
        .map(|i: u32| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

/// Base migrada (el esquema de `app_state.migrate`, ya portado) sin filas.
pub fn migrated(home: &TestHome) -> rusqlite::Connection {
    let conn = state::connect(&home.state_db()).unwrap();
    state::migrate(&conn, MIGRATIONS, 1_791_115_200.0).unwrap();
    conn
}

/// Siembra completa: filas, imágenes, `news-watch.json` y una configuración
/// en cadena.
pub fn seed_editions(home: &TestHome) {
    migrated(home).execute_batch(SEED).unwrap();
    let media = media_dir(home);
    std::fs::create_dir_all(&media).unwrap();
    std::fs::write(media.join(SMALL_IMAGE), b"\x89PNG\r\n\x1a\nfalsa").unwrap();
    std::fs::write(media.join(LARGE_IMAGE), large_image()).unwrap();
    std::fs::write(media.join(EMPTY_IMAGE), b"").unwrap();
    home.write(
        "news-watch.json",
        r#"{"news": [{"title": "Nuevo modelo", "url": "https://x.example"}], "checkedAt": 1791115000.5}"#,
    );
    home.write(
        "news-editions.json",
        r#"{"enabled": true, "summarizer": {"kind": "chain", "steps": [{"agent": "opencode", "model": "free"}, {"agent": "claude"}, "x"]}, "policy": {"slots": ["21:00", "08:30", "15:00"], "budgetUsd": 1, "maxSources": 12}}"#,
    );
}
