//! Upstream MCP streamable-HTTP mínimo y determinista para las pruebas de paridad.
//!
//! Solo implementa lo que los dos clientes (el proxy Rust y el SDK `mcp` de Python)
//! necesitan para completar la secuencia de la prueba:
//! - `POST /mcp` con una petición JSON-RPC: una única respuesta `application/json`.
//! - `POST /mcp` con una notificación: `202 Accepted` y cuerpo vacío.
//! - `initialize` contesta la versión pedida si el SDK Python la soporta (si no, la última) y
//!   emite `Mcp-Session-Id: fake-session`; las demás peticiones lo traen de vuelta y se
//!   registra (junto con la versión pedida en `initialize`).
//! - `/mcp-badversion`: igual, pero `initialize` contesta una versión que nadie soporta.
//! - `GET /mcp` (el SDK de Python abre ahí un stream SSE tras `notifications/initialized`):
//!   `405 Method Not Allowed`. El cliente Python lo reintenta una vez y desiste sin
//!   afectar la sesión.
//! - `DELETE /mcp` (cierre de sesión de ambos clientes): `200` vacío.
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, Response, StatusCode, body::Incoming, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::Value;
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

fn reply(status: StatusCode, body: Option<String>, session: bool) -> Response<Full<Bytes>> {
    let mut builder = Response::builder().status(status);
    if session {
        builder = builder.header("mcp-session-id", SESSION);
    }
    let bytes = match body {
        Some(text) => {
            builder = builder.header("content-type", "application/json");
            Bytes::from(text)
        }
        None => Bytes::new(),
    };
    builder.body(Full::new(bytes)).expect("respuesta HTTP")
}

/// Versiones que acepta el SDK Python 1.30.0 (`mcp/shared/version.py`): el falso devuelve
/// la pedida si está aquí, como hace un servidor MCP real.
const SUPPORTED: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// Respuesta JSON-RPC cruda. Se escribe a mano, no con `json!`, para mandar justo lo que
/// pydantic reescribe en el Python: claves del sobre y de los modelos fuera de orden,
/// `null` en campos opcionales y extras, `null` dentro de esquemas (que debe sobrevivir),
/// enteros en `float` tipado (`priority`) y decimales con otra grafía (`1.50`, `1e3`, `1e16`).
/// `bad_version` simula un upstream con una versión de protocolo que nadie soporta.
fn answer(request: &Value, bad_version: bool) -> String {
    let id = request["id"].to_string();
    let method = request["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => {
            let requested = request["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = if bad_version {
                "1999-01-01"
            } else if SUPPORTED.contains(&requested) {
                requested
            } else {
                "2025-11-25"
            };
            format!(
                r#"{{"protocolVersion":"{version}","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fake","version":"1"}},"instructions":null}}"#
            )
        }
        "tools/list" => {
            return format!(
                r#"{{"result":{{"tools":[{{"inputSchema":{{"type":"object","properties":{{"text":{{"type":"string","default":null,"x-weight":1.50,"x-max":1e16}}}}}},"name":"echo","title":null,"description":"Devuelve el texto","annotations":null}},{{"name":"fail","inputSchema":{{"type":"object"}},"x-extra":null}},{{"name":"bare","inputSchema":{{"type":"object"}}}}],"nextCursor":null}},"id":{id},"jsonrpc":"2.0"}}"#
            );
        }
        "tools/call" => match request["params"]["name"].as_str() {
            Some("echo") => {
                let text = request["params"]["arguments"]["text"].to_string();
                format!(
                    r#"{{"content":[{{"text":{text},"type":"text","annotations":null,"_meta":null}}],"structuredContent":null}}"#
                )
            }
            Some("fail") => r#"{"isError":true,"content":[{"type":"text","text":"fallo provocado","annotations":{"priority":1,"audience":null}}],"structuredContent":{"a":null,"n":1e3}}"#.into(),
            // Sin `content`: el modelo `CallToolResult` no lo acepta.
            Some("bare") => "{}".into(),
            _ => {
                return format!(
                    r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32602,"message":"Unknown tool"}}}}"#
                );
            }
        },
        "resources/list" => r#"{"resources":[{"uri":"HTTPS://Example.COM/docs/../a b","name":"doc","title":null,"size":3}]}"#.into(),
        "prompts/get" => r#"{"description":null,"messages":[{"content":{"type":"text","text":"hola","annotations":null},"role":"user"}]}"#.into(),
        _ => {
            return format!(
                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"message":"Method not found","code":-32601,"data":null}}}}"#
            );
        }
    };
    format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#)
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
    // `/mcp-badversion`: mismo servidor, pero con una versión de protocolo no soportada.
    let bad_version = request.uri().path() == "/mcp-badversion";
    if request.uri().path() != "/mcp" && !bad_version {
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
    let entry = if rpc == "initialize" {
        format!(
            "POST initialize session={session} version={}",
            message["params"]["protocolVersion"]
        )
    } else {
        format!("POST {rpc} session={session}")
    };
    log.lock().unwrap().push(entry);
    if message.get("id").is_none() {
        return Ok(reply(StatusCode::ACCEPTED, None, false));
    }
    Ok(reply(
        StatusCode::OK,
        Some(answer(&message, bad_version)),
        rpc == "initialize",
    ))
}
