//! Estáticos de `dash_dir` con la semántica útil de `SimpleHTTPRequestHandler`:
//! `Content-Type` de una tabla fija (sin `charset`, como `guess_type`),
//! `Content-Length`, `Last-Modified` en formato HTTP y 304 según
//! `If-Modified-Since` a precisión de segundo. `Cache-Control: no-store` lo
//! pone el transporte en toda respuesta, como `end_headers` del Python.
//!
//! Solo llegan aquí rutas que `router::classify` marcó `Static`; si el archivo
//! desaparece entre la clasificación y la lectura se responde el 404 JSON.
use super::{not_found, router};
use crate::{HandlerError, Reply, ReplyBody, Request};
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use std::{
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Tipo MIME por extensión (sin distinguir mayúsculas); desconocido →
/// `application/octet-stream`.
pub fn mime_for(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((_, ext)) = name.rsplit_once('.') else {
        return "application/octet-stream";
    };
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/vnd.microsoft.icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/x-wav",
        _ => "application/octet-stream",
    }
}

/// Resultado de la lectura bloqueante.
enum Loaded {
    NotModified,
    File { bytes: Vec<u8>, mtime: i64 },
}

/// Sirve el archivo de la petición. HEAD lee igual que GET: el cuerpo fija
/// `Content-Length` y hyper lo omite en el cable.
pub async fn serve(dash_dir: &Path, request: &Request) -> Result<Reply, HandlerError> {
    let Some(path) = router::static_path(&request.target) else {
        return not_found();
    };
    let full = dash_dir.join(path.trim_start_matches('/'));
    let ims = if_modified_since(&request.headers);
    let loaded = tokio::task::spawn_blocking(move || load(&full, ims))
        .await
        .map_err(|_| HandlerError::Failure)?;
    match loaded {
        Ok(Loaded::NotModified) => Ok(Reply {
            cache: crate::ReplyCache::NoStore,
            status: StatusCode::NOT_MODIFIED,
            headers: HeaderMap::new(),
            body: ReplyBody::Bytes(Bytes::new()),
        }),
        Ok(Loaded::File { bytes, mtime }) => {
            let mut reply = Reply::bytes(StatusCode::OK, mime_for(&path), bytes);
            let stamp =
                HeaderValue::from_str(&http_date(mtime)).map_err(|_| HandlerError::Failure)?;
            reply.headers.insert(header::LAST_MODIFIED, stamp);
            Ok(reply)
        }
        // Ausente, directorio o ilegible en este instante: 404 JSON.
        Err(_) => not_found(),
    }
}

/// `If-Modified-Since` en segundos UTC, solo si no hay `If-None-Match` y la
/// fecha se entiende y es UTC (o sin zona), como `send_head`. El Python lee
/// la primera aparición de la cabecera.
pub(crate) fn if_modified_since(headers: &[(String, String)]) -> Option<i64> {
    if headers.iter().any(|(k, _)| k == "if-none-match") {
        return None;
    }
    let (_, value) = headers.iter().find(|(k, _)| k == "if-modified-since")?;
    parse_http_date(value)
}

fn load(path: &PathBuf, ims: Option<i64>) -> io::Result<Loaded> {
    let mut file = fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::ErrorKind::NotFound.into());
    }
    let mtime = unix_seconds(meta.modified()?);
    if ims.is_some_and(|ims| mtime <= ims) {
        return Ok(Loaded::NotModified);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)?;
    Ok(Loaded::File { bytes, mtime })
}

/// Segundos enteros desde la época, truncando hacia abajo (`time.gmtime`).
pub(crate) fn unix_seconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            if before.subsec_nanos() > 0 {
                -whole - 1
            } else {
                -whole
            }
        }
    }
}

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_LONG: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// `Sun, 04 Oct 2026 12:00:00 GMT`, como `date_time_string` del Python.
pub fn http_date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    // 1970-01-01 fue jueves.
    let weekday = DAYS[usize::try_from((days + 4).rem_euclid(7)).unwrap_or(0)];
    format!(
        "{weekday}, {day:02} {} {year:04} {:02}:{:02}:{:02} GMT",
        MONTHS[usize::try_from(month - 1).unwrap_or(0)],
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Fecha HTTP → segundos UTC, con la tolerancia de
/// `email.utils.parsedate_to_datetime`: día de semana opcional e ignorado,
/// RFC 850 (`04-Oct-26`), asctime, segundos opcionales y año de dos cifras.
/// `None` si no se entiende o si la zona no es UTC: GMT/UT/UTC/Z/+0000,
/// sin zona o `-0000` (el Python asume UTC) y zonas desconocidas cuentan
/// como UTC; EST/PDT… o desplazamientos no nulos no.
pub fn parse_http_date(value: &str) -> Option<i64> {
    let mut data: Vec<String> = value.split_whitespace().map(str::to_string).collect();
    let first = data.first()?.to_ascii_lowercase();
    if first.ends_with(',') || DAYS.iter().any(|d| d.eq_ignore_ascii_case(&first)) {
        data.remove(0);
    }
    if data.len() == 3 {
        // RFC 850: `04-Oct-26 12:00:00 GMT`.
        let parts: Vec<String> = data[0].split('-').map(str::to_string).collect();
        if parts.len() == 3 {
            data.splice(0..1, parts);
        }
    }
    if data.len() == 4 {
        // Zona pegada a la hora o ausente.
        let last = data[3].clone();
        match last.find(['+', '-']).filter(|&i| i > 0) {
            Some(i) => {
                data[3] = last[..i].to_string();
                data.push(last[i..].to_string());
            }
            None => data.push(String::new()),
        }
    }
    if data.len() < 5 {
        return None;
    }
    data.truncate(5);
    let [mut dd, mut mm, mut yy, mut tm, mut tz]: [String; 5] = data.try_into().ok()?;
    mm = mm.to_ascii_lowercase();
    if month_index(&mm).is_none() {
        std::mem::swap(&mut dd, &mut mm);
        mm = mm.to_ascii_lowercase();
    }
    let month = month_index(&mm)?;
    let dd = dd.trim_end_matches(',');
    if yy.contains(':') {
        std::mem::swap(&mut yy, &mut tm);
    }
    yy = yy.trim_end_matches(',').to_string();
    if !yy.starts_with(|c: char| c.is_ascii_digit()) {
        std::mem::swap(&mut yy, &mut tz);
    }
    let tm = tm.trim_end_matches(',');
    let day: i64 = dd.parse().ok()?;
    let mut year: i64 = yy.parse().ok()?;
    let clock: Vec<&str> = tm.split(':').collect();
    let (hour, minute, second): (i64, i64, i64) = match clock.as_slice() {
        [h, m] => (h.parse().ok()?, m.parse().ok()?, 0),
        [h, m, s] => (h.parse().ok()?, m.parse().ok()?, s.parse().ok()?),
        _ => return None,
    };
    if year < 100 {
        year += if year > 68 { 1900 } else { 2000 };
    }
    if !utc_zone(&tz) {
        return None;
    }
    let valid = (1..=days_in_month(year, month)).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..60).contains(&second);
    valid.then(|| days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

fn month_index(name: &str) -> Option<i64> {
    let short = MONTHS.iter().position(|m| m.eq_ignore_ascii_case(name));
    let long = MONTHS_LONG.iter().position(|m| *m == name);
    short.or(long).and_then(|i| i64::try_from(i + 1).ok())
}

/// Zonas que el Python acaba tratando como UTC (ver `parse_http_date`).
fn utc_zone(tz: &str) -> bool {
    let tz = tz.to_ascii_uppercase();
    const OFFSET_ZONES: [&str; 10] = [
        "AST", "ADT", "EST", "EDT", "CST", "CDT", "MST", "MDT", "PST", "PDT",
    ];
    if OFFSET_ZONES.contains(&tz.as_str()) {
        return false;
    }
    match tz.strip_prefix(['+', '-']) {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            digits.bytes().all(|b| b == b'0')
        }
        _ => true,
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Días desde 1970-01-01 (algoritmo `days_from_civil` de H. Hinnant).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inversa de `days_from_civil`: (año, mes 1–12, día 1–31).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
