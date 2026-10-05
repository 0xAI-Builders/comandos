//! Uso, cuotas y analítica (Fase 2e): la caché de límites de proveedor y las
//! rutas de este dominio. Las que leen la base de uso o sus límites declinan
//! con el carril de uso apagado (D10); GET `/providers` y
//! `/optimization/plans` no la tocan.
pub mod extensions;
pub mod guard;
/// Dueño de la importación de uso de GET `/usage/state` (D1).
pub mod import;
pub mod limits;
pub mod pane_models;
pub mod providers;
/// Motor de GET `/usage/state`.
pub mod state;
pub mod week;

use super::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb};
use crate::{HandlerError, Reply, Request};
use http::StatusCode;
use pane_models::PaneOutcome;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageRoute {
    Week,
    Extensions,
    Providers,
    OptimizationPlans,
    Accounts,
    State,
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
    Entry {
        verb: Verb::Get,
        key: Key::Path("/usage/state"),
        route: NativeRoute::Usage(UsageRoute::State),
    },
];

pub async fn answer(native: &Arc<Native>, route: UsageRoute, request: &Request) -> Answer {
    match route {
        UsageRoute::State => usage_state(native).await,
        UsageRoute::Week => week::answer(native, request).await,
        UsageRoute::Extensions => extensions::answer(native, request).await,
        UsageRoute::Providers => providers::answer_providers(native).await,
        UsageRoute::OptimizationPlans => providers::answer_plans(native).await,
        UsageRoute::Accounts => providers::answer_accounts(native, request).await,
    }
}

/// GET `/usage/state` (bin/cc-dash:8450) con todos sus efectos, de los que quien
/// responde es dueño (D1): `record_pane` (dentro de `compute`), lanzar la
/// importación, lanzar el refresco de límites, los bordes de pane,
/// `pane-models.txt` y los avisos de nivel. El único punto de declinar es el
/// primer trabajo del carril de uso; antes de él no hay efectos.
async fn usage_state(native: &Arc<Native>) -> Answer {
    // Ruta apagada (`USAGE_STATE_NATIVE`): el Python responde y es dueño.
    if !native.options().usage_state_native {
        return Err(Fault::Decline);
    }
    let reply = match state::compute(native).await {
        Ok(reply) => reply,
        Err(Fault::Decline) => {
            // El Python atiende esta vuelta y escribe sus bordes: la próxima
            // reconciliación del frente vuelve a leer las opciones de tmux (R1 b).
            native.pane_models.forget_discovery();
            return Err(Fault::Decline);
        }
        Err(other) => return Err(other),
    };
    // D1: sin tarjetas de `/state` no hay bordes ni configuraciones observadas.
    let cards = native.states_cached().await.ok();
    native.import.maybe_start(
        native.import_deps(),
        native.usage.enabled(),
        cards.as_ref().map(|c| c.items.clone()),
    );
    if native.options().usage_effects
        && !reply.live_declined
        && let Some(rows) = pane_models::live_rows(
            &reply.live_panes,
            &reply.state,
            cards.as_ref().map(|c| c.items.as_slice()),
        )
    {
        match pane_models::pane_values(native, &rows, reply.tmux_panes.as_ref()).await {
            PaneOutcome::Ready(values) => {
                let o = native.options();
                native
                    .pane_models
                    .clone()
                    .apply(
                        values.values,
                        values.file_text,
                        o.tmux.clone(),
                        o.hooks.clone(),
                    )
                    .await;
                spawn_alerts(native, values.alerts);
            }
            PaneOutcome::Skip => {}
            // `write_pane_models` lanza antes de responder: 500, con los avisos
            // de los panes anteriores ya en camino (sus hilos en el Python).
            PaneOutcome::Raises(alerts) => {
                spawn_alerts(native, alerts);
                return Err(Fault::Error(HandlerError::Failure));
            }
        }
    }
    Ok(Reply::bytes(StatusCode::OK, "application/json", reply.body))
}

/// Los avisos de nivel salen en su propia tarea (los hilos del Python): la
/// respuesta no espera a cc-notifyd.
fn spawn_alerts(native: &Arc<Native>, alerts: Vec<pane_models::TierAlert>) {
    if alerts.is_empty() {
        return;
    }
    let native = native.clone();
    tokio::spawn(async move {
        pane_models::send_alerts(&native, alerts).await;
    });
}
