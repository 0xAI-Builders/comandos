//! Cliente de cable para los tests del frente `comandos dash`: escribe la
//! petición tal cual y lee la respuesta entera (`Connection: close`), así las
//! cabeceras que se comprueban son las que viajan, también en HEAD y 304.
#![allow(dead_code)]
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

pub const WAIT: Duration = Duration::from_secs(3);

pub struct Wire {
    pub status: u16,
    /// Nombres en minúsculas, en el orden del cable.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Wire {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// `METHOD target` con `Host` local, cabeceras extra (`Nombre: valor\r\n`) y cierre.
pub async fn request(port: u16, method: &str, target: &str, extra: &str) -> Wire {
    let wire = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n"
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    timeout(WAIT, stream.read_to_end(&mut out))
        .await
        .expect("la respuesta debe cerrar la conexión")
        .unwrap();
    parse(&out)
}

fn parse(raw: &[u8]) -> Wire {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("respuesta sin fin de cabeceras");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .expect("línea de estado");
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Wire {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}
