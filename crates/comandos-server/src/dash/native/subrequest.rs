//! GET al `cc-dash` heredado desde el frente (D11): el contexto de sugerencias
//! de `/state` pide `/usage/guard` y `/usage/analytics?days=7` al MISMO
//! Python al que se reenvía. Una conexión nueva por la puerta de ritmo de
//! `forward`, `Host: 127.0.0.1:<puerto>`, el token del tablero y cierre.
use crate::dash::forward::{AbortOnDrop, connect_paced};
use bytes::Bytes;
use http::{HeaderValue, Method, header};
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use std::{net::SocketAddr, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubError {
    /// No se pudo conectar (heredado caído).
    Connect,
    /// La conexión se abrió pero no hubo una respuesta HTTP/1.1 completa.
    Protocol,
    /// El plazo venció antes de tener el cuerpo entero.
    Timeout,
}

/// `GET target` al heredado; devuelve el status y el cuerpo completo.
pub async fn get(
    addr: SocketAddr,
    token: &[u8],
    target: &str,
    timeout: Duration,
) -> Result<(u16, Bytes), SubError> {
    tokio::time::timeout(timeout, exchange(addr, token, target))
        .await
        .map_err(|_| SubError::Timeout)?
}

async fn exchange(addr: SocketAddr, token: &[u8], target: &str) -> Result<(u16, Bytes), SubError> {
    let stream = connect_paced(addr).await.map_err(|_| SubError::Connect)?;
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|_| SubError::Protocol)?;
    // Si el plazo suelta este futuro, el guardia corta el socket.
    let _connection = AbortOnDrop(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let host = HeaderValue::from_str(&format!("127.0.0.1:{}", addr.port()))
        .map_err(|_| SubError::Protocol)?;
    let mut builder = http::Request::builder()
        .method(Method::GET)
        .uri(target)
        .header(header::HOST, host)
        .header(header::CONNECTION, "close");
    if !token.is_empty() {
        let value = HeaderValue::from_bytes(token).map_err(|_| SubError::Protocol)?;
        builder = builder.header("X-Comandos-Token", value);
    }
    let request = builder
        .body(Empty::<Bytes>::new())
        .map_err(|_| SubError::Protocol)?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|_| SubError::Protocol)?;
    let status = response.status().as_u16();
    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|_| SubError::Protocol)?
        .to_bytes();
    Ok((status, body))
}
