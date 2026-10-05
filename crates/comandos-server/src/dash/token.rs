//! Token de acceso remoto del tablero, compatible con `access_token()` del
//! Python (`bin/cc-dash` 4730): `~/.claude/hooks/dash-token` con `str.strip()`;
//! si falta o queda vacío se genera `secrets.token_urlsafe(32)` y se escribe con
//! `O_WRONLY|O_CREAT|O_TRUNC` y 0600. Un único lector para el arranque, la
//! puerta de acceso del transporte, `/webterm-token` y las URL del remoto.
//!
//! El Python relee el archivo en cada petición que lo necesita: rotarlo surte
//! efecto al momento. `TokenCache` hace lo mismo con un `stat` por petición y
//! solo vuelve a leer si cambió (mtime, tamaño, inodo, dispositivo).
use comandos_core::text::strip;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Nombre del archivo en `~/.claude/hooks`.
pub const TOKEN_FILE: &str = "dash-token";

/// Ruta del token bajo el HOME dado (no se consulta el entorno).
pub fn token_path(home: &Path) -> PathBuf {
    home.join(".claude/hooks").join(TOKEN_FILE)
}

/// `access_token()` sobre `path`. Bloquea.
///
/// Errores: `InvalidData` si el archivo no es UTF-8 (el `UnicodeDecodeError`
/// del Python, cuya codificación de proceso es UTF-8); cualquier otro error de
/// E/S es la excepción sin capturar del Python. Solo «no existe» o «vacío»
/// generan uno nuevo; el directorio no se crea (el `os.open` del Python
/// tampoco).
pub fn access_token_at(path: &Path) -> io::Result<String> {
    match fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let token = strip(&text);
            if !token.is_empty() {
                return Ok(token.to_owned());
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    let token = base64_urlsafe(&random);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;
    Ok(token)
}

/// El token del arranque: como `access_token_at`, creando antes
/// `~/.claude/hooks` si falta (el Python lo crea al importar el módulo).
pub fn load_token(home: &Path) -> io::Result<Vec<u8>> {
    let path = token_path(home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    access_token_at(&path).map(String::into_bytes)
}

/// `base64.urlsafe_b64encode(b).rstrip(b"=")` (`secrets.token_urlsafe`).
pub fn base64_urlsafe(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let symbol = |index: u32| {
        char::from(
            ALPHABET
                .get((index & 0x3f) as usize)
                .copied()
                .unwrap_or(b'A'),
        )
    };
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        let symbols = chunk.len() + 1;
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i < symbols {
                out.push(symbol(n >> shift));
            }
        }
    }
    out
}

/// Identidad de una versión del archivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileKey {
    mtime_ns: i128,
    len: u64,
    ino: u64,
    dev: u64,
}

impl FileKey {
    fn of(meta: &fs::Metadata) -> FileKey {
        FileKey {
            mtime_ns: i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
            len: meta.len(),
            ino: meta.ino(),
            dev: meta.dev(),
        }
    }
}

/// El token que espera la puerta de acceso en cada petición.
pub struct TokenCache {
    /// `None`: token fijo (transportes sin archivo, pruebas de transporte).
    path: Option<PathBuf>,
    fixed: Vec<u8>,
    cached: Mutex<Option<(FileKey, Vec<u8>)>>,
    /// Lo que se compara cuando el archivo no se puede leer: aleatorio, nunca
    /// coincide con lo presentado (el Python responde 401 —`UnicodeError`— o
    /// 500 —`OSError`—; aquí siempre se deniega).
    deny: Vec<u8>,
}

impl TokenCache {
    /// `fixed` es el token del arranque (ya validado por el transporte); con
    /// `path`, manda el archivo, releído si cambia.
    pub fn new(fixed: Vec<u8>, path: Option<PathBuf>) -> io::Result<TokenCache> {
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
        Ok(TokenCache {
            path,
            fixed,
            cached: Mutex::new(None),
            deny: format!("denegado-{}", base64_urlsafe(&random)).into_bytes(),
        })
    }

    /// El token vigente: un `stat` y, si el archivo cambió (o falta), una
    /// lectura (o la generación del Python).
    pub fn current(&self) -> Vec<u8> {
        let Some(path) = &self.path else {
            return self.fixed.clone();
        };
        match fs::metadata(path) {
            Ok(meta) => {
                let key = FileKey::of(&meta);
                let cached = self.cached.lock().unwrap_or_else(|p| p.into_inner());
                if let Some((at, token)) = cached.as_ref()
                    && *at == key
                {
                    return token.clone();
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return self.deny.clone(),
        }
        match access_token_at(path) {
            Ok(token) => {
                let token = token.into_bytes();
                if let Ok(meta) = fs::metadata(path) {
                    *self.cached.lock().unwrap_or_else(|p| p.into_inner()) =
                        Some((FileKey::of(&meta), token.clone()));
                }
                token
            }
            Err(_) => self.deny.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_python_urlsafe_b64encode_without_padding() {
        assert_eq!(base64_urlsafe(b""), "");
        assert_eq!(base64_urlsafe(b"f"), "Zg");
        assert_eq!(base64_urlsafe(b"fo"), "Zm8");
        assert_eq!(base64_urlsafe(b"foo"), "Zm9v");
        assert_eq!(base64_urlsafe(&[0xfb, 0xff, 0xfe]), "-__-");
        assert_eq!(base64_urlsafe(&[0u8; 32]).len(), 43);
    }

    #[test]
    fn strips_like_python_and_regenerates_when_empty() {
        let dir = std::env::temp_dir().join(format!("cmd-token-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(TOKEN_FILE);
        // `str.strip()` quita también U+001C–U+001F y espacios Unicode.
        fs::write(&path, "\u{1f}\u{a0} abc \n\u{3000}").unwrap();
        assert_eq!(access_token_at(&path).unwrap(), "abc");
        fs::write(&path, " \n").unwrap();
        let fresh = access_token_at(&path).unwrap();
        assert_eq!(fresh.len(), 43);
        assert_eq!(fs::read_to_string(&path).unwrap(), fresh);
        fs::write(&path, b"\xff").unwrap();
        assert_eq!(
            access_token_at(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_follows_rotation_and_denies_unreadable() {
        let dir = std::env::temp_dir().join(format!("cmd-token-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(TOKEN_FILE);
        fs::write(&path, "viejo\n").unwrap();
        let cache = TokenCache::new(b"del-arranque".to_vec(), Some(path.clone())).unwrap();
        assert_eq!(cache.current(), b"viejo");
        // Rotación atómica (otro inodo), como la haría un `os.replace`.
        let tmp = dir.join("nuevo.tmp");
        fs::write(&tmp, "nuevo").unwrap();
        fs::rename(&tmp, &path).unwrap();
        assert_eq!(cache.current(), b"nuevo");
        fs::write(&path, b"\xff\xfe").unwrap();
        let denied = cache.current();
        assert!(denied.starts_with(b"denegado-"));
        fs::remove_file(&path).unwrap();
        let generated = cache.current();
        assert_eq!(generated.len(), 43);
        assert_eq!(fs::read(&path).unwrap(), generated);
        let fixed = TokenCache::new(b"fijo".to_vec(), None).unwrap();
        assert_eq!(fixed.current(), b"fijo");
        let _ = fs::remove_dir_all(&dir);
    }
}
