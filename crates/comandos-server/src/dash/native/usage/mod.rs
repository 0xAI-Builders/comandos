//! Uso, cuotas y analítica (Fase 2e): la caché de límites de proveedor y las
//! rutas de este dominio. Todas declinan con el carril de uso apagado (D10).
pub mod extensions;
pub mod limits;
pub mod pane_models;
pub mod week;

use super::{Answer, Entry, Key, Native, NativeRoute, Verb};
use crate::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageRoute {
    Week,
    Extensions,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Path("/analytics/week"),
        route: NativeRoute::Usage(UsageRoute::Week),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Path("/extension-usage"),
        route: NativeRoute::Usage(UsageRoute::Extensions),
    },
];

pub async fn answer(native: &Native, route: UsageRoute, request: &Request) -> Answer {
    match route {
        UsageRoute::Week => week::answer(native, request).await,
        UsageRoute::Extensions => extensions::answer(native, request).await,
    }
}
