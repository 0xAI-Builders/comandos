//! `_suggestion_context` (6953): `guard`, rutas seleccionables y latencia
//! medida, recalculados cuando tienen más de 60 s (`now - at > 60`).
//!
//! Todo se calcula en el frente (Tarea 5a de la 2e): `guard` es
//! `token_guard_with_forecast` y la latencia, `experiment_analytics(USAGE_DB, 7)`,
//! ambos por el carril de uso (`usage::guard`); las rutas, `selectable_routes`.
//! La excepción del Python es su `except` (`{}` o tabla vacía). Lo que el frente
//! no puede reproducir con certeza (error SQL, valor no decodificable, la caché
//! de límites aún vacía, el carril apagado) declina: GET `/state` se reenvía,
//! como con el heredado caído de la 2d, y el fallo se recuerda unos segundos
//! para no repetir el cómputo en cada sondeo (R3).
use super::{StateFault, serial::Serial, suggest::SuggestContext};
use crate::dash::native::{Native, NativeOptions, usage::guard};
use comandos_runtime::{accounts, providers};
use serde_json::Value;
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
/// Cuánto se recuerda un fallo: esos cómputos declinan sin volver a
/// calcular (R3).
const FAILURE_MEMORY_MS: i64 = 5_000;
/// `socket.create_connection(..., timeout=0.3)` de `proxy_alive` (3697).
const PROXY_TIMEOUT: Duration = Duration::from_millis(300);
/// Plazo de cada consulta del contexto (guardia, que incluye la espera de la
/// primera lectura de límites, y latencia): el de la subconsulta al heredado
/// de la 2d (`SUBREQUEST_TIMEOUT`). Un carril de uso ocupado por otro trabajo
/// largo no alarga GET `/state` más que antes: al vencer, declina.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

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

    /// El contexto vigente o uno nuevo; `Err(Decline)` si el frente no puede
    /// reproducir lo que vería el Python.
    pub async fn get(
        &self,
        native: &Native,
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
        // del proxy): si fallan, no se lee la base ni se piden los límites.
        let routes = match routes(native.options(), &native.states.serial, registry.clone()).await {
            Ok(routes) => routes,
            Err(fault) => {
                self.remember_failure(now_ms);
                return Err(fault);
            }
        };
        // Cualquier fallo del carril (también un pánico del trabajo, que el
        // `except` del Python no explica) o un plazo vencido declina: nunca un
        // 500 que el Python no daría. El trabajo ya encolado en el carril
        // termina solo y su resultado se descarta (no tiene efectos).
        let Ok(Ok(guard)) =
            tokio::time::timeout(QUERY_TIMEOUT, guard::token_guard_with_forecast(native)).await
        else {
            self.remember_failure(now_ms);
            return Err(StateFault::Decline);
        };
        let Ok(Ok(latency)) = tokio::time::timeout(QUERY_TIMEOUT, guard::latency(native)).await
        else {
            self.remember_failure(now_ms);
            return Err(StateFault::Decline);
        };
        let ctx = Arc::new(SuggestContext {
            guard: guard.unwrap_or_else(|| Value::Object(serde_json::Map::new())),
            routes,
            latency,
        });
        self.failed_at.store(NO_FAILURE, Ordering::Release);
        *ready = Some((now_ms, ctx.clone()));
        Ok(ctx)
    }
}

/// `{c["id"] for c in capability_matrix() if c["selectable"]}`: cuentas
/// (`public_accounts`; su error es el `except` → conjunto vacío), binarios
/// con `providers::which`, `cc-model-proxy` con `shutil.which` (A5) y el
/// sondeo del proxy (async, 300 ms). Lo que el frente no puede reproducir
/// con certeza declina.
async fn routes(
    opts: &NativeOptions,
    serial: &Serial,
    registry: Value,
) -> Result<BTreeSet<String>, StateFault> {
    let repo = opts.repo_root.clone().ok_or(StateFault::Decline)?;
    let port = serial
        .run(move || Ok(providers::proxy_port(&repo)?))
        .await?;
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
    serial
        .run(move || {
            let Ok(discovered) =
                accounts::public_accounts(&registry, &accounts::Paths::new(&home, &cwd))
            else {
                return Ok(BTreeSet::new());
            };
            let available = |name: &str| providers::which(name, search.as_deref(), &home).is_some();
            let installed = providers::which_path("cc-model-proxy", search.as_deref()).is_some();
            let facts = providers::runtime_facts(
                &registry,
                &discovered,
                &available,
                &home,
                installed,
                alive,
            )?;
            Ok(providers::selectable_routes(&registry, &facts)?)
        })
        .await
}
