//! Reenvío al `cc-dash` Python heredado.
//!
//! Una conexión TCP nueva por petición (sin pool: el Python abre un hilo por
//! conexión y cerrarla al terminar es lo más parecido a un navegador). Se
//! reenvían método, target tal cual, cabeceras en orden salvo las hop-by-hop
//! (con `Host` intacto) y el cuerpo crudo. Nunca se añaden `X-Forwarded-For`,
//! `Via` ni `X-Real-IP`: el Python ve las mismas cabeceras que vio la puerta.
//!
//! El `Reply` vuelve en cuanto llegan las cabeceras del heredado, así que el
//! `handler_timeout` del transporte solo cubre hasta ahí. El cuerpo se bombea
//! desde una tarea propia con su límite de inactividad (`BODY_IDLE`).
use crate::{HandlerError, Reply, ReplyBody, Request};
use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use http_body::Body as _;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, client::conn::http1};
use hyper_util::rt::TokioIo;
use std::{io, net::SocketAddr, time::Duration};
use tokio::{
    net::TcpStream,
    sync::{Semaphore, mpsc},
    task::JoinHandle,
    time::timeout,
};

/// Máximo sin recibir un trozo del cuerpo heredado antes de cortar.
pub const BODY_IDLE: Duration = Duration::from_secs(60);
/// Trozos en vuelo entre el heredado y el cliente; acota la memoria.
const CHANNEL_FRAMES: usize = 8;
/// Ritmo de conexiones nuevas hacia el heredado. El `socketserver` del Python escucha con
/// backlog 5 y acepta cada conexión en un milisegundo largo; más de cinco SYN pendientes a
/// la vez esperan la retransmisión (≈ 1 s). Cada conexión nueva ocupa una plaza durante
/// `CONNECT_PACE` tras conectar: como mucho cuatro conexiones por cada 20 ms, que el Python
/// acepta sin llenar la cola, y la ráfaga la encola este proceso en microsegundos.
const CONNECT_SLOTS: usize = 4;
const CONNECT_PACE: Duration = Duration::from_millis(20);
static CONNECT_GATE: Semaphore = Semaphore::const_new(CONNECT_SLOTS);
pub const UNAVAILABLE: &str = "Servidor heredado no disponible";

/// Cabeceras de un solo salto: no cruzan el proxy en ningún sentido.
pub fn is_hop_by_hop(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "connection" | "keep-alive" | "transfer-encoding" | "te" | "trailer" | "upgrade"
    ) || name.starts_with("proxy-")
}

/// Reenvía `request` a `legacy` y devuelve la respuesta en cuanto llegan sus
/// cabeceras. Sin heredado (o si se cae antes de responder): 502.
pub async fn relay(legacy: SocketAddr, request: Request) -> Result<Reply, HandlerError> {
    let (method, target) = (request.method.clone(), request.target.clone());
    let outgoing = outgoing(request)?;
    // El semáforo nunca se cierra: `acquire` solo falla si se cerrara.
    let Ok(slot) = CONNECT_GATE.acquire().await else {
        return unavailable();
    };
    let stream = match TcpStream::connect(legacy).await {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("dash: 502 {method} {target}: sin heredado en {legacy}: {err}");
            return unavailable();
        }
    };
    // La plaza se libera pasado el ritmo, no al terminar la petición: un long-poll de 25 s
    // no debe retener conexiones nuevas.
    tokio::spawn(async move {
        tokio::time::sleep(CONNECT_PACE).await;
        drop(slot);
    });
    let _ = stream.set_nodelay(true);
    let Ok((mut sender, connection)) = http1::handshake(TokioIo::new(stream)).await else {
        eprintln!("dash: 502 {method} {target}: el heredado no habla HTTP/1.1");
        return unavailable();
    };
    // Si el transporte abandona este futuro (timeout), el guardia corta el socket.
    let connection = AbortOnDrop(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let response = match sender.send_request(outgoing).await {
        Ok(response) => response,
        Err(err) => {
            eprintln!("dash: 502 {method} {target}: el heredado cerró sin responder: {err}");
            return unavailable();
        }
    };
    drop(sender);
    let (parts, body) = response.into_parts();
    // Longitud conocida solo cuando el cuerpo va delimitado por Content-Length;
    // los flujos del Python (hasta el cierre) quedan sin longitud.
    let length = if body_allowed(parts.status) {
        body.size_hint().exact()
    } else {
        None
    };
    let headers = response_headers(&parts.headers, length.is_some());
    let (tx, receiver) = mpsc::channel(CHANNEL_FRAMES);
    tokio::spawn(pump(body, tx, connection));
    let body = match length {
        Some(length) => ReplyBody::SizedStream { length, receiver },
        None => ReplyBody::Stream(receiver),
    };
    Ok(Reply {
        status: parts.status,
        headers,
        body,
    })
}

fn unavailable() -> Result<Reply, HandlerError> {
    Reply::json(
        StatusCode::BAD_GATEWAY,
        &serde_json::json!({"error": UNAVAILABLE}),
    )
}

/// 1xx, 204 y 304 no llevan cuerpo: su `Content-Length` no enmarca nada.
fn body_allowed(status: StatusCode) -> bool {
    !(status.is_informational()
        || status == StatusCode::NO_CONTENT
        || status == StatusCode::NOT_MODIFIED)
}

fn outgoing(request: Request) -> Result<http::Request<Full<Bytes>>, HandlerError> {
    let mut builder = http::Request::builder()
        .method(request.method)
        .uri(request.target.as_str());
    let headers = builder.headers_mut().ok_or(HandlerError::Failure)?;
    for (name, value) in &request.headers {
        if is_hop_by_hop(name) {
            continue;
        }
        // El transporte mapeó cada byte a un carácter Latin-1: se deshace.
        let raw: Vec<u8> = value
            .chars()
            .map(|c| u8::try_from(u32::from(c)))
            .collect::<Result<_, _>>()
            .map_err(|_| HandlerError::Failure)?;
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| HandlerError::Failure)?;
        let value = HeaderValue::from_bytes(&raw).map_err(|_| HandlerError::Failure)?;
        headers.append(name, value);
    }
    builder
        .body(Full::new(request.body))
        .map_err(|_| HandlerError::Failure)
}

/// Cabeceras del heredado en su orden, sin hop-by-hop. `Connection: close` se
/// conserva (el Python lo usa para cortar flujos y rechazos); `Content-Length`
/// solo cuando enmarca el cuerpo, y el transporte reescribe su valor.
fn response_headers(source: &HeaderMap, sized: bool) -> HeaderMap {
    let mut headers = HeaderMap::with_capacity(source.len());
    for (name, value) in source {
        if name == header::CONNECTION {
            let closes = value.to_str().is_ok_and(|v| {
                v.split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("close"))
            });
            if closes && !headers.contains_key(header::CONNECTION) {
                headers.append(header::CONNECTION, HeaderValue::from_static("close"));
            }
            continue;
        }
        if name == header::CONTENT_LENGTH && !sized {
            continue;
        }
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        headers.append(name.clone(), value.clone());
    }
    headers
}

/// Copia los trozos del heredado al canal según llegan. Un corte o
/// inactividad del heredado se entrega como error para que el transporte
/// aborte la conexión del cliente en vez de cerrar la respuesta como completa.
async fn pump(mut body: Incoming, tx: mpsc::Sender<io::Result<Bytes>>, _connection: AbortOnDrop) {
    loop {
        let next = tokio::select! {
            // El cliente se fue: se suelta todo y el socket heredado se cierra.
            () = tx.closed() => return,
            next = timeout(BODY_IDLE, body.frame()) => next,
        };
        let data = match next {
            Ok(None) => return,
            Ok(Some(Ok(frame))) => match frame.into_data() {
                Ok(data) if !data.is_empty() => data,
                _ => continue,
            },
            Ok(Some(Err(error))) => {
                let _ = tx.send(Err(io::Error::other(error))).await;
                return;
            }
            Err(_) => {
                let _ = tx
                    .send(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "heredado inactivo",
                    )))
                    .await;
                return;
            }
        };
        if tx.send(Ok(data)).await.is_err() {
            return;
        }
    }
}

/// Aborta la tarea de la conexión heredada al soltarse.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
