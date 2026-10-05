//! GET `/providers` (8286, `provider_public_state` 1722), GET
//! `/optimization/plans` (8291, `optimization_plans` 1463) y GET `/accounts`
//! (8272): el registro de proveedores con su estado público y la matriz de
//! capacidades, los planes de optimización y el menú del botón «Cuenta».
//!
//! Lo que el Python convierte en 500 (`providers.json invalido: …` o una
//! excepción sin capturar) depende de mensajes de excepciones que el frente no
//! reproduce: se declina y responde el Python (C3). Ninguna de las tres tiene
//! efectos salvo el refresco de límites de `/accounts`, que se lanza solo cuando
//! ya no se declina.
use super::super::{
    Answer, Fault, Native, NativeOptions,
    files::{self, Strict},
    light::{error, read_reply},
    query::Query,
};
use crate::Request;
use comandos_runtime::{
    Unsure, accounts,
    model_catalog::catalog_paths,
    providers::{self, RegistryCache},
};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{net::SocketAddr, sync::Mutex, time::Duration};

/// `socket.create_connection(..., timeout=0.3)` de `proxy_alive` (3697).
const PROXY_TIMEOUT: Duration = Duration::from_millis(300);

/// `{"error": ...}` del botón «Cuenta» para un CLI sin cuentas.
const NO_ACCOUNTS: &str = "Ese CLI no maneja cuentas";

/// El texto del respaldo de una variante de plan que no valida.
const PLAN_UNAVAILABLE: &str = "configuración no disponible";

fn decline(_: Unsure) -> Fault {
    Fault::Decline
}

/// `load_provider_registry()` en un hilo de bloqueo: lectura, validación e
/// hidratación con el catálogo de modelos. Cualquier fallo declina.
fn load_registry(opts: &NativeOptions, cache: &Mutex<RegistryCache>) -> Result<Value, Fault> {
    let repo = opts.repo_root.as_ref().ok_or(Fault::Decline)?;
    let catalog = catalog_paths(
        &opts.home,
        &opts.cwd,
        opts.codex_home.as_deref(),
        opts.grok_home.as_deref(),
    );
    cache
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .load(&repo.join("config/providers.json"), &catalog)
        .map_err(decline)
}

/// Lo que `provider_public_state` y `capability_matrix` leen del sistema.
struct Base {
    registry: Value,
    /// `provider_registry.public_state(registry)`.
    public: Value,
    /// `account_registry.public_accounts(registry)`.
    discovered: Value,
    /// `shutil.which("cc-model-proxy")`.
    installed: bool,
    /// `grok_state.models()`, solo si hay harness `grok`.
    grok: Option<Result<Vec<Value>, Unsure>>,
    /// Puerto de `load_proxy_cfg()`.
    port: u16,
}

/// Registro, estado público, cuentas, binarios y modelos de Grok (todo
/// bloqueante) y, ya en el runtime, el sondeo async del proxy.
async fn base(native: &Native) -> Result<(Base, bool), Fault> {
    let opts = native.options().clone();
    let cache = native.registry.clone();
    let base = tokio::task::spawn_blocking(move || -> Result<Base, Fault> {
        let registry = load_registry(&opts, &cache)?;
        let repo = opts.repo_root.as_ref().ok_or(Fault::Decline)?;
        let port = providers::proxy_port(repo).map_err(decline)?;
        let (home, search) = (&opts.home, opts.search_path.as_deref());
        let available = |name: &str| providers::which(name, search, home).is_some();
        let public = providers::public_state(&registry, &available, home).map_err(decline)?;
        // `AccountError` ya es `[]` dentro; otro error es la excepción del Python.
        let discovered =
            accounts::public_accounts(&registry, &accounts::Paths::new(home, &opts.cwd))
                .map_err(|_| Fault::Decline)?;
        let installed = providers::which_path("cc-model-proxy", search).is_some();
        let has_grok = public
            .get("harnesses")
            .and_then(|h| h.get("grok"))
            .is_some_and(|g| !g.is_null());
        // `GROK_HOME` del proceso o `~/.grok`; relativo al directorio de trabajo.
        let grok = has_grok.then(|| {
            let dir = opts.grok_home.clone().unwrap_or_else(|| home.join(".grok"));
            providers::grok_models(&opts.cwd.join(dir))
        });
        Ok(Base {
            registry,
            public,
            discovered,
            installed,
            grok,
            port,
        })
    })
    .await
    .map_err(|_| Fault::Decline)??;
    let alive = proxy_alive(base.port).await;
    Ok((base, alive))
}

/// `proxy_alive()`: conexión TCP a `127.0.0.1:<puerto>` con plazo de 300 ms.
async fn proxy_alive(port: u16) -> bool {
    tokio::time::timeout(
        PROXY_TIMEOUT,
        tokio::net::TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port))),
    )
    .await
    .is_ok_and(|connected| connected.is_ok())
}

/// `provider_public_state()`.
pub async fn public_state(native: &Native) -> Result<Value, Fault> {
    let (base, alive) = base(native).await?;
    let grok = base.grok;
    let models = move || match &grok {
        Some(Ok(models)) => Ok(models.clone()),
        _ => Err(Unsure),
    };
    providers::complete_public_state(
        &base.registry,
        base.public,
        &base.discovered,
        &models,
        base.installed,
        alive,
    )
    .map_err(decline)
}

/// `capability_matrix()` con su registro: estado público recién calculado y
/// hechos de ejecución.
async fn registry_and_matrix(native: &Native) -> Result<(Value, Vec<Value>), Fault> {
    let (base, alive) = base(native).await?;
    let facts = providers::public_runtime_facts(
        &base.registry,
        &base.public,
        &base.discovered,
        base.installed,
        alive,
    )
    .map_err(decline)?;
    let matrix = providers::evaluate_capability_matrix(&base.registry, &facts).map_err(decline)?;
    Ok((base.registry, matrix))
}

/// `capability_matrix()`.
pub async fn capability_matrix(native: &Native) -> Result<Vec<Value>, Fault> {
    Ok(registry_and_matrix(native).await?.1)
}

/// GET `/providers`: 200 con el estado público; cualquier excepción del Python
/// (500 `providers.json invalido: …`) declina.
pub async fn answer_providers(native: &Native) -> Answer {
    read_reply(&public_state(native).await?)
}

/// `json.load(open(path))` cuyo fallo es el `except` del Python: `None`.
/// Lo incierto declina.
async fn read_json_or_none(path: std::path::PathBuf) -> Result<Option<Value>, Fault> {
    let read = tokio::task::spawn_blocking(move || files::read_json_strict(&path))
        .await
        .map_err(|_| Fault::Decline)?;
    match read {
        Strict::Value(value) => Ok(Some(value)),
        Strict::Missing | Strict::Unreadable => Ok(None),
        Strict::Unsure => Err(Fault::Decline),
    }
}

/// GET `/optimization/plans`: los planes de `config/optimization-plans.json`
/// con sus variantes validadas contra la matriz, y el perfil activo.
pub async fn answer_plans(native: &Native) -> Answer {
    let repo = native.options().repo_root.clone().ok_or(Fault::Decline)?;
    let raw = read_json_or_none(repo.join("config/optimization-plans.json"))
        .await?
        .unwrap_or_else(|| json!({"plans": []}));
    // `raw.get("plans")` fuera del `try`: un JSON que no es objeto lanza.
    let raw = raw.as_object().ok_or(Fault::Decline)?;
    let (registry, matrix) = registry_and_matrix(native).await?;
    let mut plans = Vec::new();
    for plan in py_iter(raw.get("plans").unwrap_or(&Value::Null))? {
        plans.push(plan_item(&registry, &matrix, &plan)?);
    }
    // `optimization_state().get("profile") or ""`.
    let state = read_json_or_none(native.options().hooks.join("optimization-default.json"))
        .await?
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let profile = state.get("profile").filter(|p| truthy(p));
    let active = profile.cloned().unwrap_or_else(|| json!(""));
    read_reply(&json!({"plans": plans, "active": active}))
}

/// `for x in (value or [])` del Python: lista, o claves/caracteres; lo demás
/// lanza (declina).
fn py_iter(value: &Value) -> Result<Vec<Value>, Fault> {
    if !truthy(value) {
        return Ok(Vec::new());
    }
    match value {
        Value::Array(items) => Ok(items.clone()),
        Value::String(s) => Ok(s.chars().map(|c| Value::String(c.into())).collect()),
        Value::Object(map) => Ok(map.keys().map(|k| Value::String(k.clone())).collect()),
        _ => Err(Fault::Decline),
    }
}

fn truthy(value: &Value) -> bool {
    comandos_core::json::truthy(value)
}

/// Un plan: `{id, label, icon, intent}` con `plan.get` y sus variantes.
fn plan_item(registry: &Value, matrix: &[Value], plan: &Value) -> Result<Value, Fault> {
    // `plan.get(...)` de algo que no es objeto lanza fuera del `try`.
    let plan = plan.as_object().ok_or(Fault::Decline)?;
    let mut item = Map::new();
    for key in ["id", "label", "icon", "intent"] {
        item.insert(key.into(), plan.get(key).cloned().unwrap_or(Value::Null));
    }
    let mut variants = Map::new();
    let listed = plan.get("variants").unwrap_or(&Value::Null);
    if truthy(listed) {
        for (harness, selection) in listed.as_object().ok_or(Fault::Decline)? {
            // `{**selection, ...}` del respaldo falla si no es un `dict`: la
            // excepción escapa del `except`.
            let fields = selection.as_object().ok_or(Fault::Decline)?;
            let (selectable, reason) =
                match providers::validate_selection(registry, matrix, selection, "session_model")
                    .map_err(decline)?
                {
                    // La celda validada es seleccionable: `reason` es `None`.
                    Ok(cell) => (
                        truthy(cell.get("selectable").unwrap_or(&Value::Null)),
                        cell_message(&cell)?,
                    ),
                    Err(_) => (false, json!(PLAN_UNAVAILABLE)),
                };
            let mut variant = fields.clone();
            variant.insert("selectable".into(), selectable.into());
            variant.insert("reason".into(), reason);
            variants.insert(harness.clone(), Value::Object(variant));
        }
    }
    item.insert("variants".into(), Value::Object(variants));
    Ok(Value::Object(item))
}

/// `(cell.get("reason") or {}).get("message", "")`.
fn cell_message(cell: &Value) -> Result<Value, Fault> {
    let reason = cell.get("reason").unwrap_or(&Value::Null);
    if !truthy(reason) {
        return Ok(json!(""));
    }
    let reason = reason.as_object().ok_or(Fault::Decline)?;
    Ok(reason.get("message").cloned().unwrap_or_else(|| json!("")))
}

/// GET `/accounts?harness=&usage=`: las cuentas de un CLI (`list_accounts`)
/// y, salvo con `usage=0` exacto, su menú con los límites de la caché del
/// frente (`account_menu`). D10: con el carril de uso apagado declina.
pub async fn answer_accounts(native: &Native, request: &Request) -> Answer {
    if !native.usage.enabled() {
        return Err(Fault::Decline);
    }
    let query = Query::parse(&request.target)?;
    let harness = query.first("harness").unwrap_or("claude").to_owned();
    let no_usage = query.all("usage") == ["0"];
    let opts = native.options().clone();
    let cache = native.registry.clone();
    let listed = {
        let harness = harness.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Vec<Value>>, Fault> {
            let registry = load_registry(&opts, &cache)?;
            if !has_accounts(&registry, &harness)? {
                return Ok(None);
            }
            // `AccountError` (y lo demás) fuera de un `try`: el 500 del Python
            // no se reproduce con certeza.
            accounts::list_accounts(
                &registry,
                &harness,
                &accounts::Paths::new(&opts.home, &opts.cwd),
            )
            .map(Some)
            .map_err(|_| Fault::Decline)
        })
        .await
        .map_err(|_| Fault::Decline)??
    };
    let Some(discovered) = listed else {
        return error(StatusCode::NOT_FOUND, NO_ACCOUNTS);
    };
    if no_usage {
        return read_reply(&json!({"harness": harness, "accounts": discovered}));
    }
    // Las filas de `usage_provider_limits()`; su refresco se lanza abajo.
    let rows: Vec<Value> = native
        .limits
        .current()
        .rows
        .into_iter()
        .map(Value::Object)
        .collect();
    let menu = accounts::account_menu(&Value::Array(discovered), &Value::Array(rows), &harness)
        .map_err(|_| Fault::Decline)?;
    let reply = read_reply(&json!({"harness": harness, "accounts": menu}))?;
    // Ya no se declina: `usage_provider_limits()` lanza el refresco si venció.
    let _ = native.limits.get(&native.refresh_deps());
    Ok(reply)
}

/// `((registry.get("harnesses") or {}).get(harness) or {})` y
/// `(spec.get("capabilities") or {}).get("accounts")`; un intermedio que no es
/// objeto lanzaría (declina).
fn has_accounts(registry: &Value, harness: &str) -> Result<bool, Fault> {
    fn object(value: Option<&Value>) -> Result<Option<&Map<String, Value>>, Fault> {
        match value {
            Some(value) if truthy(value) => value.as_object().map(Some).ok_or(Fault::Decline),
            _ => Ok(None),
        }
    }
    let Some(harnesses) = object(registry.get("harnesses"))? else {
        return Ok(false);
    };
    let Some(spec) = object(harnesses.get(harness))? else {
        return Ok(false);
    };
    let Some(caps) = object(spec.get("capabilities"))? else {
        return Ok(false);
    };
    Ok(caps.get("accounts").is_some_and(truthy))
}
