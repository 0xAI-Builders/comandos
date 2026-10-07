//! Una conexión a `app-state.sqlite3` para todas las rutas nativas, con la
//! puerta de esquema: si el Python migró a una versión que este binario no
//! conoce, se rechaza (y el frente reenvía todo al heredado).
use super::EventFactsFactory;
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
    /// La base tiene un esquema sin versión que este binario no reconoce
    /// (p. ej. otras columnas): como `Newer`, se rechaza para siempre.
    Incompatible(String),
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
            Refusal::Incompatible(detail) => format!(
                "comandos dash: {} tiene un esquema desconocido ({detail}); \
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

impl Refusal {
    /// La línea de un carril: nombra la base y las rutas que dejan de ser nativas.
    pub fn lane_message(&self, path: &Path, routes: &str) -> String {
        let detail = match self {
            Refusal::Newer { found, known } => {
                format!("tiene esquema {found} y este binario conoce hasta {known}")
            }
            Refusal::Unopened(error) => format!("no se pudo abrir: {error}"),
            Refusal::Incompatible(detail) => format!("tiene un esquema desconocido: {detail}"),
            Refusal::Retired => "su worker se retiró tras un fallo".to_owned(),
        };
        format!(
            "comandos dash: {}: {detail}; {routes} se reenvían al heredado",
            path.display()
        )
    }
}

pub fn known_versions() -> BTreeSet<i64> {
    MIGRATIONS.iter().map(|m| m.version).collect()
}

pub struct StateBackend {
    pub conn: Connection,
    unified: bool,
    /// Se crea al primer uso: su `import_done` es el `_EVENTS_V2_LEGACY` del Python.
    events: Option<CachedEvents>,
    /// `focus_progress.ensure_policy` de `pomodoro_store()` ya se hizo en este
    /// proceso (el Python lo repite por hilo; es idempotente).
    pub(crate) pomodoro_policy: bool,
}

enum CachedEvents {
    Native(EventRoutes<NativeFacts>),
    Injected(EventRoutes<Box<dyn crate::events_routes::Facts + Send>>),
}

impl StateBackend {
    /// Como `open_state` del runtime (3 intentos ante `SQLITE_BUSY`), pero la
    /// puerta va ANTES de migrar: una base más nueva nunca se toca.
    pub fn open(path: &Path, now_seconds: f64) -> Result<Self, Refusal> {
        let mut last = String::new();
        for attempt in 0..3u64 {
            let location = comandos_store::migrate::resolve_configured(path)
                .map_err(|e| resolution_refusal(path, e.to_string()))?;
            let unified = matches!(location, comandos_store::migrate::DbLocation::Unified(_));
            let conn = state::connect(path).map_err(|e| Refusal::Unopened(e.to_string()))?;
            let backend = Self {
                conn,
                unified,
                events: None,
                pomodoro_policy: false,
            };
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
        let uv = comandos_store::usage::schema_version(&self.conn)
            .map_err(|e| Refusal::Unopened(e.to_string()))?;
        if uv > comandos_store::usage::SCHEMA_VERSION {
            return Err(Refusal::Newer {
                found: uv,
                known: comandos_store::usage::SCHEMA_VERSION,
            });
        }
        let known = if self.unified {
            state::UNIFIED_MIGRATIONS
                .iter()
                .map(|m| m.version)
                .collect()
        } else {
            known_versions()
        };
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
        home: &Path,
        legacy: &Path,
        request: &Request,
    ) -> Result<Option<Reply>, Unanswered> {
        self.events_with_facts(home, legacy, request, None)
    }

    pub(super) fn events_with_facts(
        &mut self,
        home: &Path,
        legacy: &Path,
        request: &Request,
        factory: Option<&EventFactsFactory>,
    ) -> Result<Option<Reply>, Unanswered> {
        let routes = self.events.get_or_insert_with(|| match factory {
            None => CachedEvents::Native(EventRoutes::new_domain(
                home.to_path_buf(),
                legacy.to_path_buf(),
                NativeFacts,
            )),
            Some(make) => CachedEvents::Injected(EventRoutes::new_domain(
                home.to_path_buf(),
                legacy.to_path_buf(),
                make(),
            )),
        });
        match routes {
            CachedEvents::Native(routes) => routes.handle_native(&self.conn, request),
            CachedEvents::Injected(routes) => routes.handle_native(&self.conn, request),
        }
    }
}

// Conserva la clase de rechazo de los carriles anteriores, con un probe que
// no abre SQLite ni SHM en la fuente rechazada.
pub(super) fn resolution_refusal(path: &Path, error: String) -> Refusal {
    if !error.contains("versión más nueva") {
        return Refusal::Unopened(error);
    }
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => return Refusal::Unopened(error),
        }
    };
    let Some(home) = path.parent() else {
        return Refusal::Unopened(error);
    };
    let probe = comandos_store::unified::with_readonly_db(home, &path, |c| {
        let uv: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if uv > 11 && uv != 1000 {
            return Ok(Some((uv, 11)));
        }
        let has: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='schema_migrations')",
            [],
            |r| r.get(0),
        )?;
        if !has {
            return Ok(None);
        }
        let versions = c
            .prepare("SELECT version FROM schema_migrations")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let unified = versions.iter().any(|v| (100..=104).contains(v));
        let known = if unified { 104 } else { 11 };
        Ok(versions
            .into_iter()
            .filter(|v| *v > known && *v != 1000)
            .max()
            .map(|found| (found, known)))
    });
    match probe {
        Ok(Some((found, known))) => Refusal::Newer { found, known },
        _ => Refusal::Unopened(error),
    }
}
