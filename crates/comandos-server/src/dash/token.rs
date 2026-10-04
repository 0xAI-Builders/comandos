//! Token de acceso remoto del tablero, compatible con `access_token()` del Python:
//! `~/.claude/hooks/dash-token`, recortado; si falta o está vacío se genera una
//! vez con 32 bytes aleatorios en base64 URL-safe sin relleno y permisos 0600.
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Ruta del token bajo el HOME dado (no se consulta el entorno).
pub fn token_path(home: &Path) -> PathBuf {
    home.join(".claude/hooks/dash-token")
}

/// Lee el token o lo crea. Igual que el Python, solo «no existe» o «vacío»
/// provocan la generación; cualquier otro error de lectura se propaga.
pub fn load_token(home: &Path) -> io::Result<Vec<u8>> {
    let path = token_path(home);
    match fs::read(&path) {
        Ok(raw) => {
            let text = std::str::from_utf8(&raw).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "dash-token no es UTF-8")
            })?;
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.as_bytes().to_vec());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let token = token_urlsafe_32()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(&token)?;
    Ok(token)
}

/// Equivalente a `secrets.token_urlsafe(32)`: 43 caracteres.
fn token_urlsafe_32() -> io::Result<Vec<u8>> {
    let mut random = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    Ok(base64_urlsafe_unpadded(&random))
}

fn base64_urlsafe_unpadded(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let group = u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
        // Un bloque de n bytes produce n + 1 caracteres sin relleno.
        for index in 0..=chunk.len() {
            out.push(ALPHABET[(group >> (18 - 6 * index) & 0x3f) as usize]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64_urlsafe_unpadded;

    #[test]
    fn encodes_like_python_urlsafe_b64encode_without_padding() {
        assert_eq!(base64_urlsafe_unpadded(b""), b"");
        assert_eq!(base64_urlsafe_unpadded(b"f"), b"Zg");
        assert_eq!(base64_urlsafe_unpadded(b"fo"), b"Zm8");
        assert_eq!(base64_urlsafe_unpadded(b"foo"), b"Zm9v");
        assert_eq!(base64_urlsafe_unpadded(&[0xfb, 0xff, 0xfe]), b"-__-");
        assert_eq!(base64_urlsafe_unpadded(&[0u8; 32]).len(), 43);
    }
}
