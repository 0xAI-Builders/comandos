//! Una conexión a `app-state.sqlite3` para todas las rutas nativas, con la
//! puerta de esquema: si el Python migró a una versión que este binario no
//! conoce, se rechaza (y el frente reenvía todo al heredado).
use crate::{
    Reply, Request,
    events_routes::{EventRoutes, NativeFacts, Unanswered},
};
use comandos_store::state::{self, MIGRATIONS};
use rusqlite::Connection;
use std::{collections::BTreeSet, path::Path, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// La base tiene migraciones aplicadas que este binario no conoce.
    Newer { found: i64, known: i64 },
    /// No se pudo abrir, consultar o migrar.
    Unopened(String),
    /// El worker de la base se retiró (un trabajo entró en pánico): el
    /// backend no se reutiliza y nunca se vuelve a abrir.
    Retired,
}

impl Refusal {
    pub fn message(&self, path: &Path) -> String {
        match self {
            Refusal::Newer { found, known } => format!(
                "comandos dash: {} tiene esquema {found} y este binario conoce hasta {known}; \
                 rutas nativas desactivadas, todo se reenvía al heredado",
                path.display()
            ),
            Refusal::Unopened(error) => format!(
                "comandos dash: no se pudo abrir {}: {error}; \
                 rutas nativas desactivadas, todo se reenvía al heredado",
                path.display()
            ),
            Refusal::Retired => format!(
                "comandos dash: el worker de {} se retiró tras un fallo; \
                 rutas nativas desactivadas, todo se reenvía al heredado",
                path.display()
            ),
        }
    }
}

pub fn known_versions() -> BTreeSet<i64> {
    MIGRATIONS.iter().map(|m| m.version).collect()
}

pub struct StateBackend {
    pub conn: Connection,
    /// Se crea al primer uso: su `import_done` es el `_EVENTS_V2_LEGACY` del Python.
    events: Option<EventRoutes<NativeFacts>>,
}

impl StateBackend {
    /// Como `open_state` del runtime (3 intentos ante `SQLITE_BUSY`), pero la
    /// puerta va ANTES de migrar: una base más nueva nunca se toca.
    pub fn open(path: &Path, now_seconds: f64) -> Result<Self, Refusal> {
        let mut last = String::new();
        for attempt in 0..3u64 {
            let conn = state::connect(path).map_err(|e| Refusal::Unopened(e.to_string()))?;
            let backend = Self { conn, events: None };
            backend.admit()?;
            match state::migrate(&backend.conn, MIGRATIONS, now_seconds) {
                Ok(_) => return Ok(backend),
                Err(comandos_store::Error::Sql(error)) if attempt < 2 => {
                    last = error.to_string();
                    drop(backend);
                    std::thread::sleep(Duration::from_millis(50 * (attempt + 1)));
                }
                Err(error) => return Err(Refusal::Unopened(error.to_string())),
            }
        }
        Err(Refusal::Unopened(last))
    }

    /// Se evalúa antes de cada trabajo: cuesta una consulta a una tabla de
    /// una docena de filas (sin caché de sentencias: el rusqlite del
    /// workspace no activa la feature `cache`).
    pub fn admit(&self) -> Result<(), Refusal> {
        let known = known_versions();
        let found = self
            .conn
            .prepare("SELECT version FROM schema_migrations")
            .and_then(|mut stmt| {
                stmt.query_map([], |r| r.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| Refusal::Unopened(e.to_string()))?
            .into_iter()
            .filter(|v| !known.contains(v))
            .max();
        match found {
            None => Ok(()),
            Some(found) => Err(Refusal::Newer {
                found,
                known: known.last().copied().unwrap_or(0),
            }),
        }
    }
}

impl StateBackend {
    /// Las cuatro rutas de eventos y marcas, en el hilo del worker.
    pub fn events(
        &mut self,
        legacy: &Path,
        request: &Request,
    ) -> Result<Option<Reply>, Unanswered> {
        let routes = self
            .events
            .get_or_insert_with(|| EventRoutes::new(legacy.to_path_buf(), NativeFacts));
        routes.handle_native(&self.conn, request)
    }
}
