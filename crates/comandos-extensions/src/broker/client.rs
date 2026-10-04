//! Cliente fino de `serve`: se registra en el broker y copia bytes stdin⇄socket. Toda la
//! traducción la hace el daemon. Si el broker no está, `connect` falla y `serve` vuelve al
//! proxy directo.
use super::{check_private_dir, read_line};
use crate::Result;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    net::UnixStream,
};

/// Plazo para conectar con el socket del broker.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// Plazo para la respuesta al `attach` (incluye lanzar el upstream si no corría).
const REPLY_TIMEOUT: Duration = Duration::from_secs(1);

/// Conecta y pide `name`. `Ok` solo tras `{"ok":true}`; cualquier otra cosa es `Err`.
pub async fn connect(socket: &Path, name: &str) -> Result<BufReader<UnixStream>> {
    let dir = socket.parent().ok_or("Ruta de socket inválida")?;
    check_private_dir(dir).map_err(|e| e.to_string())?;
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(socket))
        .await
        .map_err(|_| "El broker no responde")?
        .map_err(|e| e.to_string())?;
    let mut stream = BufReader::new(stream);
    let attach = format!("{}\n", json!({"attach": name}));
    stream
        .write_all(attach.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut line = Vec::new();
    let read = tokio::time::timeout(REPLY_TIMEOUT, read_line(&mut stream, &mut line)).await;
    if !matches!(read, Ok(Ok(true))) {
        return Err("El broker no respondió al attach".into());
    }
    let reply: Value =
        serde_json::from_slice(&line).map_err(|_| "Respuesta del broker inválida")?;
    if reply["ok"] == true {
        return Ok(stream);
    }
    Err(reply["error"]
        .as_str()
        .unwrap_or("El broker rechazó el attach")
        .into())
}

/// Copia stdin→broker y broker→stdout. Al cerrar stdin se cierra la mitad de escritura y
/// se sigue leyendo; cuando el broker cierra (cliente desconectado o upstream muerto)
/// termina en seguida, aunque stdin siga abierto.
pub async fn relay(stream: BufReader<UnixStream>) -> Result<()> {
    let (mut from_broker, mut to_broker) = tokio::io::split(stream);
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let upward = async {
        let _ = tokio::io::copy(&mut stdin, &mut to_broker).await;
        let _ = to_broker.shutdown().await;
        std::future::pending::<()>().await
    };
    let downward = async {
        let copied = tokio::io::copy(&mut from_broker, &mut stdout).await;
        let _ = stdout.flush().await;
        copied
    };
    tokio::select! {
        copied = downward => copied.map(drop).map_err(|e| e.to_string()),
        () = upward => Ok(()),
    }
}
