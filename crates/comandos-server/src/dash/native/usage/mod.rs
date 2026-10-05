//! Uso, cuotas y analítica (Fase 2e): la caché de límites de proveedor y las
//! rutas de este dominio. Las que leen la base de uso o sus límites declinan
//! con el carril de uso apagado (D10); GET `/providers` y
//! `/optimization/plans` no la tocan.
pub mod extensions;
pub mod guard;
pub mod limits;
pub mod pane_models;
pub mod providers;
pub mod week;

use super::{Answer, Entry, Key, Native, NativeRoute, Verb};
use crate::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageRoute {
    Week,
    Extensions,
    Providers,
    OptimizationPlans,
    Accounts,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Path("/analytics/week"),
        route: NativeRoute::Usage(UsageRoute::Week),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Path("/providers"),
        route: NativeRoute::Usage(UsageRoute::Providers),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Path("/optimization/plans"),
        route: NativeRoute::Usage(UsageRoute::OptimizationPlans),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Path("/accounts"),
        route: NativeRoute::Usage(UsageRoute::Accounts),
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
        UsageRoute::Providers => providers::answer_providers(native).await,
        UsageRoute::OptimizationPlans => providers::answer_plans(native).await,
        UsageRoute::Accounts => providers::answer_accounts(native, request).await,
    }
}
