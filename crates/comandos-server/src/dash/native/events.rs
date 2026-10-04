//! B. Eventos y marcas. La lógica (y su paridad con el Python) vive en
//! `events_routes.rs`; aquí solo se monta sobre el worker de la base.
use super::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb, query::Query};
use crate::{HandlerError, Request};
use http::Method;

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Path("/work-marks"),
        route: NativeRoute::Events,
    },
    Entry {
        verb: Verb::Get,
        key: Key::Path("/events/v2"),
        route: NativeRoute::Events,
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/work-marks"),
        route: NativeRoute::Events,
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/events/v2"),
        route: NativeRoute::Events,
    },
];

pub async fn answer(native: &Native, request: &Request) -> Answer {
    // Antes de tocar la base (GET /events/v2 importa el events.jsonl
    // heredado): si el `urlsplit` portado ve otra ruta que la tabla o la
    // consulta trae U+FFFD, se reenvía al Python.
    if request.method == Method::GET {
        Query::parse(&request.target)?;
    }
    let legacy = native.options().hooks.join("events.jsonl");
    let request = request.clone();
    match native
        .with_state(move |backend| backend.events(&legacy, &request))
        .await?
    {
        Ok(Some(reply)) => Ok(reply),
        // La tabla y `EventRoutes` reconocen las mismas cuatro rutas.
        Ok(None) => Err(Fault::Error(HandlerError::Failure)),
        Err(error) => Err(error.into()),
    }
}
