//! Líneas JSONL acotadas para sockets de cliente y pipes de trabajadores.
use comandos_core::json::{response_dumps, response_dumps_compact};
use serde_json::Value;
use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Incluye el salto de línea final, igual que el broker Python.
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Message(Vec<u8>),
    TooLarge,
    Eof,
}

/// Drena hasta el salto final una línea excesiva sin retenerla en memoria.
pub async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
) -> io::Result<Line> {
    let mut oversized = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if oversized {
                Line::TooLarge
            } else if buffer.is_empty() {
                Line::Eof
            } else {
                Line::Message(std::mem::take(buffer))
            });
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        if !oversized {
            if count > MAX_MESSAGE.saturating_sub(buffer.len()) {
                oversized = true;
                buffer.clear();
                buffer.shrink_to(8192);
            } else {
                let needed = buffer.len() + count;
                if buffer.capacity() < needed {
                    let capacity = needed
                        .max(buffer.capacity().saturating_mul(2))
                        .min(MAX_MESSAGE);
                    buffer.reserve_exact(capacity - buffer.len());
                }
                if let Some(bytes) = available.get(..count) {
                    buffer.extend_from_slice(bytes);
                }
            }
        }
        reader.consume(count);
        if newline.is_some() {
            return Ok(if oversized {
                Line::TooLarge
            } else {
                Line::Message(std::mem::take(buffer))
            });
        }
    }
}

pub fn encode_message(value: &Value) -> Result<Vec<u8>, String> {
    let mut message = response_dumps_compact(value)?.into_bytes();
    message.push(b'\n');
    Ok(message)
}

pub fn encode_status(value: &Value) -> Result<String, String> {
    response_dumps(value)
}
