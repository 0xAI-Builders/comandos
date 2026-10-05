//! Ayudas de prueba del corte `news` (2f-4): ediciones sembradas en el
//! app-state del HOME temporal (las filas que dejaría `_store`), imágenes en
//! `$XDG_STATE_HOME/comandos/news-media` del mismo HOME y la configuración en
//! `~/.claude/hooks`. Nada de red: ninguna fuente se descarga.
#![allow(dead_code)]
use super::TestHome;
use comandos_runtime::acp_client::{Session, agent_specs};
use comandos_store::state::{self, MIGRATIONS};
use serde_json::{Value, json};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

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

// ---------------------------------------------------------------- agente ACP falso (Tarea 3)

/// Agente ACP falso (`sh`, generado por la prueba en su HOME; no es un archivo
/// del repositorio). Lee JSON-RPC por líneas y las anota en `FAKEACP_LOG`;
/// `session/new` → `{"sessionId": "s1"}`; `session/prompt` → con
/// `FAKEACP_PERMISSION=1` pide permiso para `rm -rf /` y anota la respuesta,
/// espera `FAKEACP_SLEEP` segundos, con `FAKEACP_FLOOD=chunks` manda 2 MiB de
/// texto en cuatro trozos, manda `FAKEACP_CHUNK` (un literal JSON de texto) y
/// cierra el turno. La primera línea del registro es `PID <pid> <$0>`.
pub const FAKE_ACP: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
log=${FAKEACP_LOG:?}
printf 'PID %s %s\n' "$$" "$0" >> "$log"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -n 's/^{"jsonrpc": "2.0", "id": \([0-9]*\), "method": .*/\1/p')
  case "$line" in
    *'"method": "session/new"'*)
      printf '{"jsonrpc": "2.0", "id": %s, "result": {"sessionId": "s1"}}\n' "$id" ;;
    *'"method": "session/prompt"'*)
      if [ "${FAKEACP_PERMISSION:-}" = 1 ]; then
        printf '%s\n' '{"jsonrpc": "2.0", "id": 900, "method": "session/request_permission", "params": {"sessionId": "s1", "options": [{"optionId": "si", "kind": "allow_once"}, {"optionId": "no", "kind": "reject_once"}], "toolCall": {"title": "rm -rf /"}}}'
        IFS= read -r answer
        printf '%s\n' "$answer" >> "$log"
      fi
      sleep "${FAKEACP_SLEEP:-0}"
      if [ "${FAKEACP_FLOOD:-}" = chunks ]; then
        i=0; while [ $i -lt 4 ]; do
          printf '{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "'
          head -c 524288 /dev/zero | tr '\0' a
          printf '"}}}}\n'
          i=$((i+1))
        done
      fi
      printf '{"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": %s}}}}\n' "$FAKEACP_CHUNK"
      printf '{"jsonrpc": "2.0", "id": %s, "result": {"stopReason": "end_turn"}}\n' "$id" ;;
  esac
done
printf 'EOF\n' >> "$log"
"#;

/// `news-editions.json` con el único agente de las pruebas.
pub const FAKE_CHAIN: &str =
    r#"{"enabled": true, "summarizer": {"kind": "acp", "agent": "falso"}}"#;

/// El agente falso instalado en un HOME: el guion, su registro de líneas y un
/// `config/providers.json` de prueba (`<HOME>/fake-repo`) en el que TODO
/// agente ACP (también `claude`, `codex`, `grok`, `opencode`, `agy`) es el
/// guion falso, más el agente `falso`. El frente lo lee por `opts.repo_root`;
/// el Python del oráculo, por `prelude()` (P58: `PROVIDERS_FILE` es fijo).
pub struct FakeAcp {
    pub script: PathBuf,
    pub log: PathBuf,
    pub repo: PathBuf,
}

impl FakeAcp {
    pub fn at(home: &TestHome) -> FakeAcp {
        FakeAcp {
            script: home.root.join("fake-acp"),
            log: home.root.join("fake-acp.log"),
            repo: home.root.join("fake-repo"),
        }
    }

    /// Instala el guion y el registro; `env` va al entorno del agente.
    pub fn install(home: &TestHome, env: &[(&str, &str)]) -> FakeAcp {
        let fake = FakeAcp::at(home);
        std::fs::write(&fake.script, FAKE_ACP).unwrap();
        std::fs::set_permissions(&fake.script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut agent_env = serde_json::Map::new();
        agent_env.insert("FAKEACP_LOG".into(), json!(fake.log.display().to_string()));
        for (key, value) in env {
            agent_env.insert((*key).into(), json!(value));
        }
        let spec = json!({"label": "Falso (ACP)", "command": [fake.script.display().to_string()], "env": agent_env});
        let text = std::fs::read_to_string(super::repo().join("config/providers.json")).unwrap();
        let mut registry: Value = serde_json::from_str(&text).unwrap();
        let acp = registry["acpAgents"].as_object_mut().unwrap();
        for value in acp.values_mut() {
            *value = spec.clone();
        }
        acp.insert("falso".into(), spec);
        registry["harnesses"]["falso"] = json!({"label": "Falso"});
        registry["motors"]["falso"] = json!({"label": "Falso", "modelMatch": "^falso$"});
        assert_eq!(
            comandos_runtime::providers::validate_registry(&registry),
            Ok(Ok(())),
            "registro de prueba inválido"
        );
        std::fs::create_dir_all(fake.repo.join("config")).unwrap();
        std::fs::write(
            fake.repo.join("config/providers.json"),
            serde_json::to_string_pretty(&registry).unwrap(),
        )
        .unwrap();
        fake
    }

    /// Python del oráculo: `providers.load_registry` lee el registro de prueba
    /// del HOME del oráculo, y el reloj de `news_reading` es `NOW_MS`.
    pub fn prelude() -> String {
        format!(
            "import news_reading\nnews_reading._now = lambda: {now}\n\
             import providers as _laneb_providers\n\
             _laneb_load = _laneb_providers.load_registry\n\
             _laneb_providers.load_registry = lambda path=None: _laneb_load(\
             os.path.join(os.environ[\"HOME\"], \"fake-repo/config/providers.json\"))\n",
            now = super::NOW_MS
        )
    }

    /// Canario (antes de cualquier `session/prompt`): todo agente del registro
    /// es el guion falso por ruta absoluta, y el proceso que lanza el cliente
    /// ACP del frente es `sh <guion>`, que muere al cerrar la sesión.
    pub fn canary(&self, home: &TestHome) {
        let text = std::fs::read_to_string(self.repo.join("config/providers.json")).unwrap();
        let registry: Value = serde_json::from_str(&text).unwrap();
        let specs = agent_specs(&registry);
        assert!(specs.contains_key("falso"));
        for (name, spec) in &specs {
            assert_eq!(
                spec["command"],
                json!([self.script.display().to_string()]),
                "el agente {name} del registro de prueba no es el falso"
            );
        }
        let before = self.pids().len();
        let options = comandos_runtime::acp_client::OpenOptions {
            model: String::new(),
            extra_env: vec![("COMANDOS_SILENT_AGENT".into(), "1".into())],
            search_path: Some(home.root.join("bin").into_os_string()),
            home: home.root.clone(),
            base_env: Some(
                home.confined_env()
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            ),
        };
        let mut session = Session::open(&specs["falso"], &home.root.join("tmp"), &options).unwrap();
        session.new_session(10.0).unwrap();
        let pids = self.pids();
        assert_eq!(pids.len(), before + 1, "{pids:?}");
        let pid = *pids.last().unwrap();
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap();
        let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
        assert_eq!(
            args.get(1).copied(),
            Some(self.script.as_os_str().as_encoded_bytes()),
            "el proceso lanzado no es el agente falso"
        );
        session.close();
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "el agente sigue vivo"
        );
        let _ = std::fs::remove_file(&self.log);
    }

    /// PIDs de los agentes lanzados (cada uno anota el suyo al arrancar).
    pub fn pids(&self) -> Vec<u32> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.strip_prefix("PID "))
            .filter_map(|l| l.split(' ').next()?.parse().ok())
            .collect()
    }

    /// Lo que recibieron los agentes, sin los PID ni el `EOF` final (carrera
    /// entre cerrar stdin y el SIGTERM de `close`, la misma en los dos lados) y
    /// con el HOME como `<HOME>`.
    pub fn received(&self, home: &TestHome) -> Vec<String> {
        let root = home.root.display().to_string();
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.starts_with("PID ") && *l != "EOF")
            .map(|l| l.replace(&root, "<HOME>"))
            .collect()
    }

    /// Procesos vivos de este guion (por `/proc`).
    pub fn alive(&self) -> Vec<u32> {
        let needle = self.script.as_os_str().as_encoded_bytes();
        let mut out = Vec::new();
        for entry in std::fs::read_dir("/proc").unwrap().flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(cmd) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            if cmd.windows(needle.len()).any(|w| w == needle) {
                out.push(pid);
            }
        }
        out
    }
}
