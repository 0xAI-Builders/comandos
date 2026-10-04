//! Upstream MCP streamable-HTTP mínimo y determinista para las pruebas de paridad.
//!
//! Solo implementa lo que los dos clientes (el proxy Rust y el SDK `mcp` de Python)
//! necesitan para completar la secuencia de la prueba:
//! - `POST /mcp` con una petición JSON-RPC: una única respuesta `application/json`.
//! - `POST /mcp` con una notificación: `202 Accepted` y cuerpo vacío.
//! - `initialize` emite `Mcp-Session-Id: fake-session`; las demás peticiones lo traen
//!   de vuelta y se registra.
//! - `GET /mcp` (el SDK de Python abre ahí un stream SSE tras `notifications/initialized`):
//!   `405 Method Not Allowed`. El cliente Python lo reintenta una vez y desiste sin
//!   afectar la sesión.
//! - `DELETE /mcp` (cierre de sesión de ambos clientes): `200` vacío.
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, Response, StatusCode, body::Incoming, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    net::TcpListener,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

pub const SESSION: &str = "fake-session";

/// Escucha en un puerto efímero ya reservado (sin carreras) y devuelve ese puerto.
/// El hilo vive hasta que termina el proceso de pruebas.
pub fn spawn(log: Arc<Mutex<Vec<String>>>) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind del upstream falso");
    listener
        .set_nonblocking(true)
        .expect("socket no bloqueante");
    let port = listener.local_addr().expect("puerto local").port();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .expect("runtime del upstream falso");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener tokio");
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let log = log.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request| handle(request, log.clone()));
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
    });
    (port, handle)
}

fn reply(status: StatusCode, body: Option<&Value>, session: bool) -> Response<Full<Bytes>> {
    let mut builder = Response::builder().status(status);
    if session {
        builder = builder.header("mcp-session-id", SESSION);
    }
    let bytes = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Bytes::from(value.to_string())
        }
        None => Bytes::new(),
    };
    builder.body(Full::new(bytes)).expect("respuesta HTTP")
}

/// Respuesta JSON-RPC del servidor falso; claves en el orden en que las serializa el
/// SDK de Python para que la paridad no dependa de reordenar.
fn answer(request: &Value) -> Value {
    let id = request["id"].clone();
    let method = request["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "fake", "version": "1"}
        }),
        "tools/list" => json!({"tools": [
            {
                "name": "echo",
                "description": "Devuelve el texto",
                "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}
            },
            {"name": "fail", "inputSchema": {"type": "object"}}
        ]}),
        "tools/call" => match request["params"]["name"].as_str() {
            Some("echo") => json!({"content": [
                {"type": "text", "text": request["params"]["arguments"]["text"].clone()}
            ]}),
            Some("fail") => json!({
                "content": [{"type": "text", "text": "fallo provocado"}],
                "isError": true
            }),
            _ => {
                return json!({"jsonrpc": "2.0", "id": id,
                    "error": {"code": -32602, "message": "Unknown tool"}});
            }
        },
        _ => {
            return json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "Method not found"}});
        }
    };
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

async fn handle(
    request: Request<Incoming>,
    log: Arc<Mutex<Vec<String>>>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = request.method().clone();
    let session = request
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    if request.uri().path() != "/mcp" {
        log.lock()
            .unwrap()
            .push(format!("{method} {}", request.uri()));
        return Ok(reply(StatusCode::NOT_FOUND, None, false));
    }
    if method == Method::GET {
        log.lock().unwrap().push(format!("GET session={session}"));
        return Ok(reply(StatusCode::METHOD_NOT_ALLOWED, None, false));
    }
    if method == Method::DELETE {
        log.lock()
            .unwrap()
            .push(format!("DELETE session={session}"));
        return Ok(reply(StatusCode::OK, None, false));
    }
    if method != Method::POST {
        log.lock()
            .unwrap()
            .push(format!("{method} session={session}"));
        return Ok(reply(StatusCode::METHOD_NOT_ALLOWED, None, false));
    }
    let Ok(body) = request.into_body().collect().await else {
        return Ok(reply(StatusCode::BAD_REQUEST, None, false));
    };
    let Ok(message) = serde_json::from_slice::<Value>(&body.to_bytes()) else {
        return Ok(reply(StatusCode::BAD_REQUEST, None, false));
    };
    let rpc = message["method"].as_str().unwrap_or("").to_owned();
    log.lock()
        .unwrap()
        .push(format!("POST {rpc} session={session}"));
    if message.get("id").is_none() {
        return Ok(reply(StatusCode::ACCEPTED, None, false));
    }
    Ok(reply(
        StatusCode::OK,
        Some(&answer(&message)),
        rpc == "initialize",
    ))
}
