//! `_suggestion_context` (6953): `guard`, rutas seleccionables y latencia
//! medida, recalculados cuando tienen más de 60 s (`now - at > 60`).
//!
//! `guard` y la latencia salen del heredado (D1): GET `/usage/guard` es
//! exactamente `token_guard_with_forecast()` y GET `/usage/analytics?days=7`,
//! `experiment_analytics(USAGE_DB, 7)`. Solo las respuestas que el heredado da
//! cuando esa función lanza son el `except` del Python (`{}` / tabla vacía):
//! el 500 y el 504 de `_guard_request` y, en la analítica, el 400 de su
//! `except ValueError`. Cualquier otro status (401, 403, 404, 502…), heredado
//! caído, plazo vencido o un 200 que no se puede leer como el Python (p. ej.
//! un sustituto suelto): no se sabe qué vería el Python → declinar, y el fallo
//! se recuerda unos segundos para no repetir la espera en cada sondeo (R3).
//! Las rutas se calculan en Rust (`selectable_routes`).
use super::{StateFault, serial::Serial, suggest::SuggestContext, suggest::latency_from};
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
        serial: &Serial,
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
        let routes = match routes(opts, serial, registry.clone()).await {
            Ok(routes) => routes,
            Err(fault) => {
                self.remember_failure(now_ms);
                return Err(fault);
            }
        };
        let (guard, latency) = tokio::join!(
            legacy_json(opts, "/usage/guard", false),
            legacy_json(opts, "/usage/analytics?days=7", true),
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

/// `Ok(Some(valor))` con 200 y JSON legible; `Ok(None)` con la respuesta de
/// la excepción de la función (el `except` del Python); `Err` si no hay
/// respuesta, el 200 no se puede leer o el status no corresponde a esa
/// excepción. `value_error`: la ruta captura `ValueError` con un 400.
async fn legacy_json(
    opts: &NativeOptions,
    target: &str,
    value_error: bool,
) -> Result<Option<Value>, ()> {
    match subrequest::get(opts.legacy, &opts.legacy_token, target, SUBREQUEST_TIMEOUT).await {
        Ok((200, body)) => {
            let text = std::str::from_utf8(&body).map_err(|_| ())?;
            workspace_loads(text).map(Some).map_err(|_| ())
        }
        Ok((status, body)) if python_except(status, &body, value_error) => Ok(None),
        Ok(_) | Err(_) => Err(()),
    }
}

/// La respuesta del heredado cuando la función de la ruta lanza: `_fail` de
/// `_guard_request` (`{"error": "Error interno del tablero"}` con 500,
/// `{"error": "Tiempo de espera agotado"}` con 504, ambos excepciones que el
/// `except Exception` de `_suggestion_context` captura) o, si la ruta lo
/// tiene, el 400 `{"error": str(e)}` de su `except ValueError`.
fn python_except(status: u16, body: &[u8], value_error: bool) -> bool {
    let Some(Value::Object(map)) = std::str::from_utf8(body)
        .ok()
        .and_then(|text| workspace_loads(text).ok())
    else {
        return false;
    };
    let Some(Value::String(message)) = map.get("error") else {
        return false;
    };
    if map.len() != 1 {
        return false;
    }
    match status {
        500 => message == "Error interno del tablero",
        504 => message == "Tiempo de espera agotado",
        400 => value_error,
        _ => false,
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

#[cfg(test)]
mod tests {
    use super::python_except;

    #[test]
    fn only_the_function_exception_is_the_python_except() {
        let internal = br#"{"error": "Error interno del tablero"}"#;
        let timeout = br#"{"error": "Tiempo de espera agotado"}"#;
        let value = br#"{"error": "invalid literal"}"#;
        assert!(python_except(500, internal, false));
        assert!(python_except(504, timeout, false));
        assert!(python_except(400, value, true));
        // Otro status u otro cuerpo: no se sabe qué vería el Python.
        assert!(!python_except(400, value, false));
        assert!(!python_except(500, timeout, false));
        assert!(!python_except(
            500,
            br#"{"error": "Error interno del tablero", "x": 1}"#,
            false
        ));
        assert!(!python_except(401, internal, true));
        assert!(!python_except(
            403,
            br#"{"error": "Host no permitido"}"#,
            true
        ));
        assert!(!python_except(404, internal, true));
        assert!(!python_except(502, b"Bad Gateway", true));
    }
}
