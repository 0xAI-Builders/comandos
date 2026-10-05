//! `_suggestion_context` (6953): `guard`, rutas seleccionables y latencia
//! medida, recalculados cuando tienen más de 60 s (`now - at > 60`).
//!
//! `guard` y la latencia salen del heredado (D1): GET `/usage/guard` es
//! exactamente `token_guard_with_forecast()` y GET `/usage/analytics?days=7`,
//! `experiment_analytics(USAGE_DB, 7)`. Un status distinto de 200 es el
//! `except` del Python (`{}` / tabla vacía). Heredado caído, plazo vencido o
//! un 200 que no se puede leer como el Python (p. ej. un sustituto suelto): no se sabe qué vería el Python
//! → declinar, y el fallo se recuerda unos segundos para no repetir la espera
//! en cada sondeo (R3). Las rutas se calculan en Rust (`selectable_routes`).
use super::{StateFault, suggest::SuggestContext, suggest::latency_from};
use crate::dash::native::{NativeOptions, subrequest};
use comandos_core::json::workspace_loads;
use comandos_runtime::{accounts, providers};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

/// Vigencia del contexto (la condición del Python es `>`).
const MAX_AGE_MS: i64 = 60_000;
/// Plazo de cada subconsulta al heredado (R3).
const SUBREQUEST_TIMEOUT: Duration = Duration::from_secs(2);
/// Cuánto se recuerda un fallo del heredado: esos cómputos declinan sin
/// volver a esperar (R3).
const FAILURE_MEMORY_MS: i64 = 5_000;
/// `socket.create_connection(..., timeout=0.3)` de `proxy_alive` (3697).
const PROXY_TIMEOUT: Duration = Duration::from_millis(300);

/// Sin fallo recordado.
const NO_FAILURE: i64 = i64::MIN;

pub struct Context {
    ready: tokio::sync::Mutex<Option<(i64, Arc<SuggestContext>)>>,
    /// Instante (ms) del último fallo, fuera del candado: `failing` lo lee
    /// sin esperar a un cómputo del contexto en curso.
    failed_at: AtomicI64,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            ready: tokio::sync::Mutex::new(None),
            failed_at: AtomicI64::new(NO_FAILURE),
        }
    }
}

impl Context {
    /// Hay un fallo del contexto recordado en `now_ms` (R3): GET `/state`
    /// declina antes de tocar tmux o `/proc`, porque el cómputo no
    /// terminaría si una tarjeta necesitara el contexto (y el Python responde
    /// lo mismo si no lo necesita).
    pub fn failing(&self, now_ms: i64) -> bool {
        let failed = self.failed_at.load(Ordering::Acquire);
        failed != NO_FAILURE && (failed..failed.saturating_add(FAILURE_MEMORY_MS)).contains(&now_ms)
    }

    fn remember_failure(&self, now_ms: i64) {
        self.failed_at.store(now_ms, Ordering::Release);
    }

    /// El contexto vigente o uno nuevo; `Err(Decline)` si el heredado no dio
    /// una respuesta que el frente pueda reproducir.
    pub async fn get(
        &self,
        opts: &NativeOptions,
        registry: &Value,
        now_ms: i64,
    ) -> Result<Arc<SuggestContext>, StateFault> {
        let mut ready = self.ready.lock().await;
        if let Some((at, ctx)) = ready.as_ref()
            && now_ms - at <= MAX_AGE_MS
        {
            return Ok(ctx.clone());
        }
        if self.failing(now_ms) {
            return Err(StateFault::Decline);
        }
        // Las rutas primero (locales: registro, cuentas y el sondeo de 300 ms
        // del proxy): si fallan, no se pregunta al heredado, y el fallo se
        // recuerda igual que el del heredado.
        let routes = match routes(opts, registry.clone()).await {
            Ok(routes) => routes,
            Err(fault) => {
                self.remember_failure(now_ms);
                return Err(fault);
            }
        };
        let (guard, latency) = tokio::join!(
            legacy_json(opts, "/usage/guard"),
            legacy_json(opts, "/usage/analytics?days=7"),
        );
        let (Ok(guard), Ok(latency)) = (guard, latency) else {
            self.remember_failure(now_ms);
            return Err(StateFault::Decline);
        };
        let ctx = Arc::new(SuggestContext {
            guard: guard.unwrap_or_else(|| json!({})),
            routes,
            latency: latency.map(|v| latency_from(&v)).unwrap_or_default(),
        });
        self.failed_at.store(NO_FAILURE, Ordering::Release);
        *ready = Some((now_ms, ctx.clone()));
        Ok(ctx)
    }
}

/// `Ok(Some(valor))` con 200 y JSON legible; `Ok(None)` con otro status (el
/// `except` del Python); `Err` si no hay respuesta o el 200 no se puede leer.
async fn legacy_json(opts: &NativeOptions, target: &str) -> Result<Option<Value>, ()> {
    match subrequest::get(opts.legacy, &opts.legacy_token, target, SUBREQUEST_TIMEOUT).await {
        Ok((200, body)) => {
            let text = std::str::from_utf8(&body).map_err(|_| ())?;
            workspace_loads(text).map(Some).map_err(|_| ())
        }
        Ok(_) => Ok(None),
        Err(_) => Err(()),
    }
}

/// `{c["id"] for c in capability_matrix() if c["selectable"]}`: cuentas
/// (`public_accounts`; su error es el `except` → conjunto vacío), binarios
/// con `providers::which`, `cc-model-proxy` con `shutil.which` (A5) y el
/// sondeo del proxy (async, 300 ms). Lo que el frente no puede reproducir
/// con certeza declina.
async fn routes(opts: &NativeOptions, registry: Value) -> Result<BTreeSet<String>, StateFault> {
    let repo = opts.repo_root.clone().ok_or(StateFault::Decline)?;
    let port = tokio::task::spawn_blocking(move || providers::proxy_port(&repo))
        .await
        .map_err(|_| StateFault::Failure)??;
    let alive = tokio::time::timeout(
        PROXY_TIMEOUT,
        tokio::net::TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port))),
    )
    .await
    .is_ok_and(|connected| connected.is_ok());
    let (home, cwd, search) = (
        opts.home.clone(),
        opts.cwd.clone(),
        opts.search_path.clone(),
    );
    tokio::task::spawn_blocking(move || {
        let Ok(discovered) =
            accounts::public_accounts(&registry, &accounts::Paths::new(&home, &cwd))
        else {
            return Ok(BTreeSet::new());
        };
        let available = |name: &str| providers::which(name, search.as_deref(), &home).is_some();
        let installed = providers::which_path("cc-model-proxy", search.as_deref()).is_some();
        let facts =
            providers::runtime_facts(&registry, &discovered, &available, &home, installed, alive)?;
        Ok(providers::selectable_routes(&registry, &facts)?)
    })
    .await
    .map_err(|_| StateFault::Failure)?
}
