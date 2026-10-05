//! GET /extension-usage (8369, `session_profile_store.extension_usage`,
//! `lib/session_profiles.py:449`): skills y servidores MCP observados en los
//! eventos de herramientas de la base de uso, por sesión/pane y días.
//!
//! Orden del Python: validaciones (`sesión inválida`, `panel inválido`,
//! `int(days)`), después `Path(db).is_file()`; sin archivo, el resultado vacío
//! sin abrir nada (D8: la ruta no crea la base ni toca el carril). El Python
//! abre `mode=ro` y nunca migra; el carril sí (C10, el mismo cuerpo con la base
//! en v11).
use super::super::{
    Answer, Fault, Native,
    light::{error, read_reply},
    py::{self, NumError},
    query::Query,
};
use crate::{HandlerError, Request};
use comandos_store::usage_read::{self, ReadError};
use http::StatusCode;
use std::io::ErrorKind;

/// Los `errno` que `pathlib` traga en `is_file()` (`_IGNORED_ERROS` de 3.10):
/// `ENOENT`, `ENOTDIR`, `EBADF`, `ELOOP` (valores de Linux).
const IGNORED_ERRNOS: [i32; 4] = [2, 20, 9, 40];

pub async fn answer(native: &Native, request: &Request) -> Answer {
    // D10: con el carril de uso apagado el Python es dueño del dominio entero.
    if !native.usage.enabled() {
        return Err(Fault::Decline);
    }
    let query = Query::parse(&request.target)?;
    let session = query.first("session").unwrap_or("").to_owned();
    let pane = query.first("pane").unwrap_or("").to_owned();
    let days_text = query.first("days").unwrap_or("7");
    if let Some(message) = usage_read::extension_scope_error(&session, &pane) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    // `int(days)`: el `ValueError` es un 400 con su texto; lo que el port no
    // reproduce (no ASCII, fuera de `i64`, que el Python acotaría a 90) declina.
    let days = match py::int(days_text) {
        Ok(n) => n,
        Err(NumError::Invalid) => match py::int_error_message(days_text) {
            Some(message) => return error(StatusCode::BAD_REQUEST, &message),
            None => return Err(Fault::Decline),
        },
        Err(_) => return Err(Fault::Decline),
    };
    let now = (native.options().clock)() as f64 / 1000.0;
    // `Path(db).is_file()` (sigue enlaces); el `stat` fuera del hilo del runtime.
    let path = native.options().usage_db.clone();
    let stat = tokio::task::spawn_blocking(move || std::fs::metadata(path))
        .await
        .map_err(|_| Fault::Decline)?;
    match stat {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return read_reply(&usage_read::extension_usage_empty(&session, &pane, days)),
        Err(e)
            if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
                || e.raw_os_error()
                    .is_some_and(|n| IGNORED_ERRNOS.contains(&n)) =>
        {
            return read_reply(&usage_read::extension_usage_empty(&session, &pane, days));
        }
        // Otro `OSError` (p. ej. `EACCES`): el Python responde 400 con `str(e)`,
        // que no se reproduce con certeza.
        Err(_) => return Err(Fault::Decline),
    }
    let read = native
        .usage
        .with(move |u| usage_read::extension_usage(&u.conn, &session, &pane, days, now))
        .await?;
    match read {
        Ok(Ok(value)) => read_reply(&value),
        Ok(Err(message)) => error(StatusCode::BAD_REQUEST, &message),
        // Una excepción que el Python no captura: 500.
        Err(ReadError::Raises) => Err(Fault::Error(HandlerError::Failure)),
        // SQL, BLOB, texto no UTF-8 o incierto: la lectura no tuvo efectos.
        Err(ReadError::Sql(_) | ReadError::Undecodable | ReadError::Unsure) => Err(Fault::Decline),
    }
}
