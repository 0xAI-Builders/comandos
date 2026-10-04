//! `urllib.parse.parse_qs(urlsplit(self.path).query)` y `(q.get(k) or [d])[0]`.
use super::Fault;
use crate::dash::router::path_of;
use comandos_core::dashboard_access::{query_pairs, request_target_parts};

pub struct Query(Vec<(String, String)>);

impl Query {
    /// Declina si el `urlsplit` portado ve otra ruta que el corte crudo, o si
    /// la decodificación produjo U+FFFD (`errors="replace"` del Python: el
    /// texto resultante no se puede comparar con certeza).
    pub fn parse(target: &str) -> Result<Query, Fault> {
        let (path, query) = request_target_parts(target).ok_or(Fault::Decline)?;
        if path != path_of(target) {
            return Err(Fault::Decline);
        }
        let pairs = query_pairs(&query, false);
        if pairs
            .iter()
            .any(|(k, v)| k.contains('\u{fffd}') || v.contains('\u{fffd}'))
        {
            return Err(Fault::Decline);
        }
        Ok(Query(pairs))
    }

    /// Primera ocurrencia no vacía (`parse_qs` descarta las vacías).
    pub fn first(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}
