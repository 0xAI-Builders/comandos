//! cc-notifyd falso: servidor HTTP con hyper que guarda el cuerpo de cada POST.
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub type Bodies = Arc<Mutex<Vec<String>>>;

/// Escucha en `127.0.0.1:port` (0 = puerto libre) y devuelve el puerto real.
pub fn spawn(port: u16) -> (u16, JoinHandle<()>, Bodies) {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind notifyd falso");
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let bodies: Bodies = Arc::default();
    let shared = bodies.clone();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let bodies = shared.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                        let bodies = bodies.clone();
                        async move {
                            // El cc-notifyd real lee el cuerpo por Content-Length y
                            // rechaza peticiones de navegador (con Origin).
                            let headers = request.headers();
                            let problem = if request.method() != hyper::Method::POST {
                                Some(format!("método {}", request.method()))
                            } else if request.uri().path() != "/notify" {
                                Some(format!("ruta {}", request.uri().path()))
                            } else if !headers.contains_key(hyper::header::CONTENT_LENGTH) {
                                Some("sin Content-Length".to_owned())
                            } else if headers.contains_key(hyper::header::ORIGIN) {
                                Some("con Origin".to_owned())
                            } else {
                                None
                            };
                            let body = request.into_body().collect().await?.to_bytes();
                            let body = String::from_utf8_lossy(&body).into_owned();
                            bodies.lock().unwrap().push(match problem {
                                Some(problem) => format!("PETICIÓN INVÁLIDA ({problem}): {body}"),
                                None => body,
                            });
                            Ok::<_, hyper::Error>(Response::new(Full::new(Bytes::from_static(
                                b"{\"ok\":true}",
                            ))))
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
    });
    (port, handle, bodies)
}
