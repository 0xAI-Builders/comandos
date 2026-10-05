//! Cuerpos JSON grandes en trozos de 32 KiB, enviados con su longitud.
//!
//! glibc sirve con `mmap` todo bloque de más de su umbral (128 KiB al
//! arrancar) y, al soltar uno, sube el umbral de `mmap` a ese tamaño y el de
//! recorte al doble. Un cuerpo de 0,5–1 MB por petición lo dejaba en ≈ 1 MB:
//! desde entonces las arenas de los hilos no se recortaban y el Pss del frente
//! subía con cada reconstrucción. En trozos, ningún bloque pasa el umbral.
use crate::{Reply, ReplyBody};
use bytes::Bytes;

/// Tamaño de cada trozo: 32 KiB, bajo el umbral de `mmap` de glibc.
pub const CHUNK: usize = 32 * 1024;

/// Un cuerpo en trozos compartidos (clonarlo no copia los bytes).
#[derive(Clone, Default)]
pub struct ChunkedBody {
    parts: Vec<Bytes>,
    len: usize,
}

impl ChunkedBody {
    /// Los trozos de un codificador por trozos (`response_dumps_chunks`).
    pub fn from_chunks(chunks: Vec<String>) -> Self {
        let mut body = Self::default();
        for chunk in chunks {
            body.push(Bytes::from(chunk));
        }
        body
    }

    pub fn push(&mut self, part: Bytes) {
        self.len += part.len();
        self.parts.push(part);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// El cuerpo entero en un bloque (para las pruebas).
    pub fn to_vec(&self) -> Vec<u8> {
        self.parts.concat()
    }

    /// La respuesta `200 application/json` con `Content-Length`.
    pub fn reply(&self) -> Reply {
        self.clone().into_reply()
    }

    pub fn into_reply(self) -> Reply {
        let mut reply = Reply::bytes(http::StatusCode::OK, "application/json", Bytes::new());
        let (tx, receiver) = tokio::sync::mpsc::channel(self.parts.len().max(1));
        for part in self.parts {
            // Hay sitio para todos: el canal se creó con su número.
            let _ = tx.try_send(Ok(part));
        }
        reply.body = ReplyBody::SizedStream {
            length: self.len as u64,
            receiver,
        };
        reply
    }
}

impl PartialEq<&str> for ChunkedBody {
    fn eq(&self, other: &&str) -> bool {
        self.to_vec() == other.as_bytes()
    }
}

impl std::fmt::Debug for ChunkedBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChunkedBody({} bytes)", self.len)
    }
}
