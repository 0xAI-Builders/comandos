//! Clasificación de rutas del frente `comandos dash`.
//!
//! El criterio es el mismo que la puerta de `do_GET` del Python: una ruta GET
//! es API si su ruta cruda (con consulta) empieza por algún prefijo de
//! `API_GET`, salvo que sea un asset público existente (`/workspace.css`).
//! Lo que no es API se sirve como estático; el resto se reenvía al Python.
use comandos_core::dashboard_access as access;
use http::Method;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    /// GET/HEAD fuera de los prefijos API: lo sirve Rust desde `dash_dir`.
    Static,
    /// Todo lo demás: se reenvía al Python heredado.
    Forward,
}

/// `asset_exists` recibe solo la ruta sin consulta y solo tras validar la
/// forma léxica del asset, igual que el transporte: nunca ve `..` ni `%`.
pub fn classify(method: &Method, target: &str, asset_exists: &dyn Fn(&str) -> bool) -> RouteClass {
    if method != Method::GET && method != Method::HEAD {
        return RouteClass::Forward;
    }
    if !access::API_GET
        .iter()
        .any(|prefix| target.starts_with(prefix))
    {
        return RouteClass::Static;
    }
    let lexical = access::public_asset(target, true).unwrap_or(false);
    if lexical && asset_exists(path_of(target)) {
        RouteClass::Static
    } else {
        RouteClass::Forward
    }
}

/// Ruta sin consulta ni fragmento, como `Uri::path()` en el transporte.
pub fn path_of(target: &str) -> &str {
    target
        .split_once(['?', '#'])
        .map_or(target, |(path, _)| path)
}
