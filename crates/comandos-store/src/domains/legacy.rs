//! Lectores de bytes que solo consideran ausente un archivo inexistente.
use crate::Result;
use std::{fs, path::Path};
pub(crate) fn read(file: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(file) {
        Ok(body) => Ok(Some(body)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn lines(body: &[u8]) -> Vec<Vec<u8>> {
    if body.is_empty() {
        return vec![];
    }
    body.strip_suffix(b"\n")
        .unwrap_or(body)
        .split(|b| *b == b'\n')
        .map(<[u8]>::to_vec)
        .collect()
}
