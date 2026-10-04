//! Cliente fino de `serve`: se registra en el broker y copia bytes stdin⇄socket. Toda la
//! traducción la hace el daemon. Si el broker no sirve, `connect` (o `relay`, mientras no
//! se haya leído nada de stdin) falla y `serve` vuelve al proxy directo.
use super::{check_private_dir, read_line};
use crate::Result;
use serde_json::Value;
use std::{cell::Cell, path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

/// Plazo para conectar con el socket del broker.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// Plazo para la respuesta al `attach`: algo más que la espera del daemon por el
/// `initialize` de un upstream nuevo, para recibir su error en vez de cortar antes.
const REPLY_TIMEOUT: Duration = Duration::from_secs(6);
/// Tras cerrar stdin, tiempo máximo esperando a que el broker cierre.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Conecta y envía la línea `attach` (ver [`super::attach_request`]). `Ok` solo tras
/// `{"ok":true}`; cualquier otra respuesta, plazo vencido o error es `Err`.
pub async fn connect(socket: &Path, request: &Value) -> Result<BufReader<UnixStream>> {
    let dir = socket.parent().ok_or("Ruta de socket inválida")?;
    check_private_dir(dir).map_err(|e| e.to_string())?;
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(socket))
        .await
        .map_err(|_| "El broker no responde")?
        .map_err(|e| e.to_string())?;
    let mut stream = BufReader::new(stream);
    let attach = format!("{request}\n");
    let sent = stream.write_all(attach.as_bytes()).await;
    sent.map_err(|e| e.to_string())?;
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
    let error = reply["error"]
        .as_str()
        .unwrap_or("El broker rechazó el attach");
    Err(error.into())
}

/// Copia stdin→broker y broker→stdout. Devuelve el código de salida de la sesión.
///
/// `Err` solo si el broker cerró antes de que se leyera un byte de stdin y sin haber
/// escrito nada en stdout: el llamador aún puede usar el proxy directo. Después, el cierre
/// es definitivo. Al cerrar stdin se cierra la mitad de escritura y se espera al broker
/// como mucho [`DRAIN_TIMEOUT`].
pub async fn relay(stream: BufReader<UnixStream>) -> Result<i32> {
    let (mut from_broker, mut to_broker) = tokio::io::split(stream);
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let consumed = Cell::new(false);
    let upward = async {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match stdin.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    consumed.set(true);
                    if to_broker.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = to_broker.shutdown().await;
        tokio::time::sleep(DRAIN_TIMEOUT).await;
    };
    let downward = async {
        let copied = tokio::io::copy(&mut from_broker, &mut stdout).await;
        let _ = stdout.flush().await;
        copied
    };
    tokio::select! {
        copied = downward => match copied {
            Ok(0) if !consumed.get() => Err("El broker cerró antes de empezar".into()),
            Ok(_) => Ok(0),
            Err(_) if !consumed.get() => Err("El broker cerró antes de empezar".into()),
            Err(_) => Ok(1),
        },
        () = upward => Ok(0),
    }
}
