//! Semántica de send_head de Python. E/S bloqueante fuera del reactor.
use super::super::{Answer, Fault};
use crate::dash::statics;
use crate::{HandlerError, Reply, Request};
use http::{HeaderValue, StatusCode, header};
use std::{
    fs,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
const BODY_CAP: u64 = 16 * 1024 * 1024;
const ENTRY_CAP: usize = 100_000;

pub async fn serve(root: &Path, request: &Request) -> Answer {
    let root = root.to_owned();
    let request = request.clone();
    tokio::task::spawn_blocking(move || load(&root, &request))
        .await
        .map_err(|_| Fault::Error(HandlerError::Failure))?
}
fn set(
    reply: &mut Reply,
    key: http::header::HeaderName,
    value: impl ToString,
) -> Result<(), Fault> {
    reply.headers.insert(
        key,
        HeaderValue::from_str(&value.to_string()).map_err(|_| Fault::Decline)?,
    );
    Ok(())
}
fn finish(mut r: Reply) -> Answer {
    if r.status != StatusCode::NOT_MODIFIED
        && let crate::ReplyBody::Bytes(ref body) = r.body
    {
        let size = body.len();
        set(&mut r, header::CONTENT_LENGTH, size)?;
    }
    set(&mut r, header::CACHE_CONTROL, "no-store")?;
    Ok(r)
}
fn error(message: &str) -> Answer {
    let body = format!(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\"\n        \"http://www.w3.org/TR/html4/strict.dtd\">\n<html>\n    <head>\n        <meta http-equiv=\"Content-Type\" content=\"text/html;charset=utf-8\">\n        <title>Error response</title>\n    </head>\n    <body>\n        <h1>Error response</h1>\n        <p>Error code: 404</p>\n        <p>Message: {}.</p>\n        <p>Error code explanation: HTTPStatus.NOT_FOUND - Nothing matches the given URI.</p>\n    </body>\n</html>\n",
        escape(message)
    );
    let mut reply = Reply::bytes(StatusCode::NOT_FOUND, "text/html;charset=utf-8", body);
    set(&mut reply, header::CONNECTION, "close")?;
    finish(reply)
}
fn path_of(target: &str) -> &str {
    target.split(['?', '#']).next().unwrap_or("")
}
fn decode(s: &str) -> Result<String, Fault> {
    let mut out = vec![];
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b.get(i) == Some(&b'%')
            && let Some(hex) = b
                .get(i + 1..i + 3)
                .and_then(|v| std::str::from_utf8(v).ok())
                .and_then(|v| u8::from_str_radix(v, 16).ok())
        {
            out.push(hex);
            i += 3;
        } else {
            if let Some(c) = b.get(i) {
                out.push(*c);
            }
            i += 1;
        }
    }
    // Python surrogatepass puede producir nombres de bytes no UTF-8; hasta
    // portar ese caso, se deja al heredado, sin cambiar el recurso solicitado.
    if out.contains(&0)
        || out
            .windows(3)
            .any(|w| matches!(w, [0xed, 0xa0..=0xbf, 0x80..=0xbf]))
    {
        return Err(Fault::Decline);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}
fn translate(root: &Path, target: &str) -> Result<PathBuf, Fault> {
    let raw = decode(path_of(target))?;
    let mut parts = vec![];
    for p in raw.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    let mut path = root.to_owned();
    for p in parts {
        path.push(p);
    }
    if path_of(target).trim_end().ends_with('/') {
        path.push("");
    }
    Ok(path)
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn listing(path: &Path, target: &str) -> Answer {
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return error("No permission to list directory"),
    };
    let mut names = vec![];
    for e in entries {
        let e = e.map_err(|_| Fault::Decline)?;
        let name = e.file_name().into_string().map_err(|_| Fault::Decline)?;
        names.push((name, e.path()));
        if names.len() > ENTRY_CAP {
            return Err(Fault::Decline);
        }
    }
    names.sort_by_key(|(n, _)| n.to_lowercase());
    let display = escape(&decode(target)?);
    let mut body = format!(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">\n<html>\n<head>\n<meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">\n<title>Directory listing for {display}</title>\n</head>\n<body>\n<h1>Directory listing for {display}</h1>\n<hr>\n<ul>\n"
    );
    for (name, path) in names {
        let mut shown = name.clone();
        let mut link = name;
        if path.is_dir() {
            shown.push('/');
            link.push('/');
        }
        if path.is_symlink() {
            shown = shown.trim_end_matches('/').to_owned();
            shown.push('@');
        }
        body.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>\n",
            quote(&link),
            escape(&shown)
        ));
        if body.len() as u64 > BODY_CAP {
            return Err(Fault::Decline);
        }
    }
    body.push_str("</ul>\n<hr>\n</body>\n</html>\n");
    finish(Reply::bytes(
        StatusCode::OK,
        "text/html; charset=utf-8",
        body,
    ))
}
fn load(root: &Path, request: &Request) -> Answer {
    let mut path = translate(root, &request.target)?;
    if path.is_dir() {
        if !path_of(&request.target).ends_with('/') {
            let index = request
                .target
                .find(['?', '#'])
                .unwrap_or(request.target.len());
            let (p, suffix) = request.target.split_at(index);
            let mut r = Reply::bytes(StatusCode::MOVED_PERMANENTLY, "text/html", vec![]);
            r.headers.remove(header::CONTENT_TYPE);
            set(&mut r, header::LOCATION, format!("{p}/{suffix}"))?;
            return finish(r);
        }
        if let Some(index) = ["index.html", "index.htm"]
            .into_iter()
            .map(|i| path.join(i))
            .find(|p| p.is_file())
        {
            path = index;
        } else {
            return listing(&path, &request.target);
        }
    }
    // O_NONBLOCK evita que un FIFO colocado en dash agote trabajadores.
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(f) => f,
        Err(_) => return error("File not found"),
    };
    let metadata = match file.metadata() {
        Ok(m) if m.is_file() => m,
        _ => return error("File not found"),
    };
    let mtime = metadata
        .modified()
        .map(statics::unix_seconds)
        .map_err(|_| Fault::Decline)?;
    if statics::if_modified_since(&request.headers).is_some_and(|stamp| mtime <= stamp) {
        let mut reply = Reply::bytes(StatusCode::NOT_MODIFIED, "text/html", vec![]);
        reply.headers.remove(header::CONTENT_TYPE);
        return finish(reply);
    }
    if metadata.len() > BODY_CAP {
        return Err(Fault::Decline);
    }
    let mut body = vec![];
    file.by_ref()
        .take(BODY_CAP + 1)
        .read_to_end(&mut body)
        .map_err(|_| Fault::Decline)?;
    if body.len() as u64 > BODY_CAP {
        return Err(Fault::Decline);
    }
    let mut reply = Reply::bytes(
        StatusCode::OK,
        statics::mime_for(path.to_str().ok_or(Fault::Decline)?),
        body,
    );
    set(&mut reply, header::LAST_MODIFIED, statics::http_date(mtime))?;
    finish(reply)
}
