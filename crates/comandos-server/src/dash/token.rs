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
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
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
    read_token(path, &|| {}).map(|(token, _)| token)
}

/// `access_token_at` con la identidad de lo leído: el `fstat` del descriptor
/// del que se lee, tomado ANTES de leer. Un cambio concurrente (rotación por
/// `rename` o reescritura en el sitio) deja la clave vieja con lo leído, así
/// que la próxima petición ve otra clave y vuelve a leer. `None` si el token
/// se acaba de generar (no se guarda: lo relee la próxima). `between` corre
/// entre el `fstat` y la lectura (las pruebas simulan ahí la carrera).
fn read_token(path: &Path, between: &dyn Fn()) -> io::Result<(String, Option<fs::Metadata>)> {
    match fs::File::open(path) {
        Ok(mut file) => {
            let meta = file.metadata()?;
            between();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let text = String::from_utf8(bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let token = strip(&text);
            if !token.is_empty() {
                return Ok((token.to_owned(), Some(meta)));
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
    Ok((token, None))
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
        self.current_with(SystemTime::now(), &|| {})
    }

    fn current_with(&self, now: SystemTime, between: &dyn Fn()) -> Vec<u8> {
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
        match read_token(path, between) {
            Ok((token, meta)) => {
                let token = token.into_bytes();
                let mut cached = self.cached.lock().unwrap_or_else(|p| p.into_inner());
                *cached = meta
                    .filter(|meta| settled(meta, now))
                    .map(|meta| (FileKey::of(&meta), token.clone()));
                token
            }
            Err(_) => self.deny.clone(),
        }
    }
}

/// Regla «racy» de git: una versión cuya mtime está a menos de `RACY` de
/// ahora (o en el futuro) aún puede cambiar sin que cambie la clave (otra
/// escritura del mismo tamaño en el mismo tic de mtime): no se guarda y se
/// vuelve a leer hasta que envejece.
const RACY: Duration = Duration::from_secs(2);

fn settled(meta: &fs::Metadata, now: SystemTime) -> bool {
    meta.modified()
        .ok()
        .and_then(|mtime| now.duration_since(mtime).ok())
        .is_some_and(|age| age >= RACY)
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

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cmd-token-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_mtime(path: &Path, at: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    /// Tres tokens del mismo tamaño (los de verdad siempre miden 43).
    const A: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const B: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
    const C: &str = "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC";

    #[test]
    fn same_size_rotation_in_place_and_by_rename_is_seen_at_once() {
        let dir = scratch("same");
        let path = dir.join(TOKEN_FILE);
        fs::write(&path, A).unwrap();
        let cache = TokenCache::new(b"arranque".to_vec(), Some(path.clone())).unwrap();
        assert_eq!(cache.current(), A.as_bytes());
        // En el sitio: mismo inodo, mismo tamaño, quizá el mismo tic de mtime.
        fs::write(&path, B).unwrap();
        assert_eq!(cache.current(), B.as_bytes(), "el nuevo vale ya");
        let tmp = dir.join("c.tmp");
        fs::write(&tmp, C).unwrap();
        fs::rename(&tmp, &path).unwrap();
        assert_eq!(cache.current(), C.as_bytes(), "rotación atómica");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn racy_versions_are_not_cached() {
        let dir = scratch("racy");
        let path = dir.join(TOKEN_FILE);
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        fs::write(&path, A).unwrap();
        set_mtime(&path, t0);
        let cache = TokenCache::new(b"arranque".to_vec(), Some(path.clone())).unwrap();
        // A 1 s de su mtime la versión aún es «racy»: no se guarda.
        let young = t0 + Duration::from_secs(1);
        assert_eq!(cache.current_with(young, &|| {}), A.as_bytes());
        // Otra escritura del mismo tamaño en el MISMO tic: la clave no cambia,
        // pero como nada quedó guardado se vuelve a leer.
        fs::write(&path, B).unwrap();
        set_mtime(&path, t0);
        assert_eq!(cache.current_with(young, &|| {}), B.as_bytes());
        // Ya asentada (≥ 2 s) se guarda: el `stat` basta.
        let old = t0 + Duration::from_secs(10);
        assert_eq!(cache.current_with(old, &|| {}), B.as_bytes());
        assert!(cache.cached.lock().unwrap().is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    /// La carrera de la revisión: el archivo rota entre la identidad y la
    /// lectura. La clave es la del descriptor leído (tomada antes de leer), así
    /// que la próxima petición ve otra identidad y lee el nuevo.
    #[test]
    fn rotation_between_stat_and_read_is_reread_next_time() {
        let dir = scratch("between");
        let path = dir.join(TOKEN_FILE);
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let now = t0 + Duration::from_secs(10);
        fs::write(&path, A).unwrap();
        set_mtime(&path, t0);
        let cache = TokenCache::new(b"arranque".to_vec(), Some(path.clone())).unwrap();
        let rotate = || {
            let tmp = dir.join("b.tmp");
            fs::write(&tmp, B).unwrap();
            set_mtime(&tmp, t0);
            fs::rename(&tmp, &path).unwrap();
        };
        // Lo leído es el viejo (el descriptor sigue en el inodo anterior)...
        assert_eq!(cache.current_with(now, &rotate), A.as_bytes());
        // ...y la siguiente ya acepta el nuevo y rechaza el viejo.
        assert_eq!(cache.current_with(now, &|| {}), B.as_bytes());
        // En el sitio y con la mtime fresca entre la identidad y la lectura: se
        // lee el nuevo y la clave vieja obliga a releer después.
        let fresh = TokenCache::new(b"arranque".to_vec(), Some(path.clone())).unwrap();
        let in_place = || fs::write(&path, C).unwrap();
        assert_eq!(fresh.current_with(now, &in_place), C.as_bytes());
        assert_eq!(fresh.current(), C.as_bytes());
        assert_eq!(cache.current(), C.as_bytes());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_tokens_are_not_cached() {
        let dir = scratch("gen");
        let path = dir.join(TOKEN_FILE);
        let cache = TokenCache::new(b"arranque".to_vec(), Some(path.clone())).unwrap();
        let generated = cache.current();
        assert_eq!(generated.len(), 43);
        assert!(cache.cached.lock().unwrap().is_none());
        // Otra petición que generó a la vez ganó la escritura: se lee el archivo.
        fs::write(&path, A).unwrap();
        assert_eq!(cache.current(), A.as_bytes());
        let _ = fs::remove_dir_all(&dir);
    }
}
