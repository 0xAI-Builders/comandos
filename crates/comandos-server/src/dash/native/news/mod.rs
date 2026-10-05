//! Corte `news` (plan 2f-4): el lector de los Resúmenes de noticias.
//!
//! - Tarea 1 (`read`): GET `/news/latest`, `/news/editions`, `/news/edition`,
//!   `/news/media/<32hex>.<ext>`, `/news/source`, `/news/chat`, `/news/notes`
//!   y `/news/saved`. Solo lecturas: app-state por el worker de la base,
//!   archivos por el pool de bloqueo.
//! - Tarea 2 (`write`): POST `/news/saved`, `/news/notes` y `/news/chat/note`
//!   (escrituras sin agente), en su propia tarea y por el worker de la base.
pub mod read;
pub mod write;

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
    SavedPost,
    NotesPost,
    ChatNotePost,
}

const fn get(key: Key, route: NewsRoute) -> Entry {
    Entry {
        verb: Verb::Get,
        key,
        route: NativeRoute::News(route),
    }
}

const fn post(path: &'static str, route: NewsRoute) -> Entry {
    Entry {
        verb: Verb::Post,
        // `self.path in ("/news/saved", …)`: la ruta cruda exacta.
        key: Key::Raw(path),
        route: NativeRoute::News(route),
    }
}

/// Las ramas de `_do_GET` (8398–8415) y de `do_POST` (9065), con su forma de
/// comparar la ruta. POST `/news/chat` y `/news/translate` (agentes) son de
/// la Tarea 3: hasta entonces se reenvían.
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
    post("/news/saved", NewsRoute::SavedPost),
    post("/news/notes", NewsRoute::NotesPost),
    post("/news/chat/note", NewsRoute::ChatNotePost),
];

pub async fn answer(native: &Arc<Native>, route: NewsRoute, request: &Request) -> Answer {
    match route {
        NewsRoute::SavedPost | NewsRoute::NotesPost | NewsRoute::ChatNotePost => {
            write::answer(native, route, request).await
        }
        _ => read::answer(native, route, request).await,
    }
}
