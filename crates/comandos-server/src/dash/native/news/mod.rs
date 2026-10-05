//! Corte `news` (plan 2f-4): el lector de los Resúmenes de noticias.
//!
//! - Tarea 1 (`read`): GET `/news/latest`, `/news/editions`, `/news/edition`,
//!   `/news/media/<32hex>.<ext>`, `/news/source`, `/news/chat`, `/news/notes`
//!   y `/news/saved`. Solo lecturas: app-state por el worker de la base,
//!   archivos por el pool de bloqueo.
pub mod read;

use super::{Answer, Entry, Key, Native, NativeRoute, Verb};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsRoute {
    Latest,
    Editions,
    Edition,
    Media,
    Source,
    Chat,
    Notes,
    Saved,
}

const fn get(key: Key, route: NewsRoute) -> Entry {
    Entry {
        verb: Verb::Get,
        key,
        route: NativeRoute::News(route),
    }
}

/// Las ramas de `_do_GET` (8398–8415), con su forma de comparar la ruta.
pub const ROUTES: &[Entry] = &[
    // `self.path.startswith("/news/latest")`.
    get(Key::Prefix("/news/latest"), NewsRoute::Latest),
    get(Key::Path("/news/editions"), NewsRoute::Editions),
    // `urlsplit(self.path).path.startswith("/news/media/")`: la ruta cruda
    // empieza igual exactamente cuando la ruta de `urlsplit` lo hace.
    get(Key::Prefix("/news/media/"), NewsRoute::Media),
    get(Key::Path("/news/source"), NewsRoute::Source),
    get(Key::Path("/news/chat"), NewsRoute::Chat),
    get(Key::Path("/news/notes"), NewsRoute::Notes),
    get(Key::Path("/news/saved"), NewsRoute::Saved),
    get(Key::Path("/news/edition"), NewsRoute::Edition),
];

pub async fn answer(native: &Arc<Native>, route: NewsRoute, request: &Request) -> Answer {
    read::answer(native, route, request).await
}
