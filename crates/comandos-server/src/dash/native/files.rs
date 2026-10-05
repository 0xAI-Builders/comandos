//! Archivos JSON de `~/.claude/hooks` como los lee y escribe `bin/cc-dash`.
use comandos_core::json::{MAX_WORKSPACE_JSON_DEPTH, response_dumps, workspace_loads};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs, io,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

pub enum Strict {
    /// `FileNotFoundError`.
    Missing,
    /// El Python cae en su `except` con certeza: JSON roto o texto con BOM
    /// (`json.loads` de un `str` que empieza por U+FEFF: «Unexpected UTF-8 BOM»).
    Unreadable,
    /// No se sabe con certeza qué leería el Python: otro error de E/S, bytes
    /// que no son UTF-8 (dependen de la codificación del proceso), sustitutos
    /// sueltos o anidamiento que el parser portado rechaza y `json` acepta.
    /// Quien lo reciba declina.
    Unsure,
    Value(Value),
}

/// `json.load(open(path))` del Python: lectura en modo texto (UTF-8), así que
/// un BOM o UTF-16 no se decodifican como en `json.loads(bytes)`.
pub fn read_json_strict(path: &Path) -> Strict {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Strict::Missing,
        Err(_) => return Strict::Unsure,
    };
    match std::str::from_utf8(&bytes) {
        Ok(text) => loads_strict(text),
        Err(_) => Strict::Unsure,
    }
}

/// `json.loads(texto)` sobre un `str` ya decodificado, con la misma
/// clasificación que `read_json_strict` (BOM → ilegible; sustitutos sueltos o
/// anidamiento profundo → incierto).
pub fn loads_strict(text: &str) -> Strict {
    if text.starts_with('\u{feff}') {
        return Strict::Unreadable;
    }
    match workspace_loads(text) {
        Ok(value) => Strict::Value(value),
        Err(_) if has_surrogate_escape(text) || deep(text) => Strict::Unsure,
        Err(_) => Strict::Unreadable,
    }
}

/// `\uD800`–`\uDFFF`: el `json` del Python los admite sueltos; el Rust no.
fn has_surrogate_escape(text: &str) -> bool {
    text.match_indices("\\u").any(|(i, _)| {
        let hex = text.as_bytes().get(i + 2..i + 4).unwrap_or_default();
        matches!(hex, [b'd' | b'D', b'8'..=b'9' | b'a'..=b'f' | b'A'..=b'F'])
    })
}

/// El límite de anidamiento del parser portado no es el de CPython.
fn deep(text: &str) -> bool {
    text.bytes().filter(|b| matches!(b, b'[' | b'{')).count() >= MAX_WORKSPACE_JSON_DEPTH
}

/// `write_json_file` (5090) → `write_file_atomic` (5063): temporal en el mismo
/// directorio, fsync, permisos del archivo previo (0600 si es nuevo) y rename.
/// Bloquea: llamar dentro de `spawn_blocking`.
pub fn write_json_atomic(path: &Path, value: &Value) -> io::Result<()> {
    let text = response_dumps(value).map_err(io::Error::other)?;
    write_text_atomic(path, &text)
}

/// `write_file_atomic` (5063) con texto ya formado.
/// Bloquea: llamar dentro de `spawn_blocking`.
pub fn write_text_atomic(path: &Path, text: &str) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mode = match fs::metadata(path) {
        Ok(meta) => meta.permissions().mode() & 0o7777,
        Err(e) if e.kind() == io::ErrorKind::NotFound => 0o600,
        Err(e) => return Err(e),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("ruta sin nombre"))?
        .to_string_lossy()
        .into_owned();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = dir.join(format!(".{name}.{suffix}.tmp"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// `file_lock` (5168): `flock` exclusivo sobre `<ruta>.lock` (creado 0600),
/// el mismo que toman el Python y cc-app. `try_acquire` lo pide sin esperar
/// (`Ok(None)` si otro lo tiene; quien llama declina antes de leer o escribir
/// nada); `acquire` espera como el Python. Se suelta al soltar el valor
/// (cerrar el descriptor suelta el `flock`).
pub struct FileLock {
    _file: fs::File,
}

impl FileLock {
    pub fn try_acquire(path: &Path) -> io::Result<Option<FileLock>> {
        let file = lock_file_for(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(FileLock { _file: file })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(e)) => Err(e),
        }
    }

    /// `fcntl.flock(fd, LOCK_EX)` con espera: el `file_lock` del Python tal
    /// cual. Bloquea hasta que el otro dueño (Python, cc-app) lo suelte, sin
    /// plazo: solo para hilos propios. Las rutas usan `acquire_timeout`.
    pub fn acquire(path: &Path) -> io::Result<FileLock> {
        let file = lock_file_for(path)?;
        file.lock()?;
        Ok(FileLock { _file: file })
    }

    /// `acquire` desde el runtime, con plazo. Los que esperan la misma ruta
    /// hacen cola en un `Mutex` asíncrono, así que a lo sumo UN hilo de
    /// bloqueo espera el `flock` de cada ruta (un dueño colgado no hace crecer
    /// el `spawn_blocking`). Vencido `limit` devuelve `TimedOut` (quien llama
    /// declina antes de cualquier efecto); el hilo que ya esperaba sigue en
    /// cola con su turno y suelta el candado en cuanto lo obtiene.
    pub async fn acquire_timeout(path: &Path, limit: Duration) -> io::Result<FileLock> {
        let deadline = tokio::time::Instant::now() + limit;
        let slot = waiter_slot(path);
        let turn = tokio::time::timeout_at(deadline, Arc::clone(&slot.turn).lock_owned())
            .await
            .map_err(|_| timed_out(path))?;
        let target = path.to_path_buf();
        let waiting = slot.clone();
        let blocked = tokio::task::spawn_blocking(move || {
            // El turno vive en el hilo: mientras este espere el `flock`, nadie
            // más abre otro hilo para la misma ruta.
            let _turn = turn;
            waiting.blocked.fetch_add(1, Ordering::AcqRel);
            let lock = FileLock::acquire(&target);
            waiting.blocked.fetch_sub(1, Ordering::AcqRel);
            lock
        });
        match tokio::time::timeout_at(deadline, blocked).await {
            Ok(Ok(lock)) => lock,
            Ok(Err(join)) => Err(io::Error::other(join.to_string())),
            Err(_) => Err(timed_out(path)),
        }
    }

    /// Hilos de bloqueo que esperan ahora el `flock` de `path` (0 o 1).
    pub fn blocked_waiters(path: &Path) -> usize {
        waiter_slot(path).blocked.load(Ordering::Acquire)
    }
}

/// Plazo por omisión de `acquire_timeout` en las rutas: el Python espera sin
/// plazo, pero un dueño colgado no puede dejar la petición abierta siempre.
pub const LOCK_WAIT: Duration = Duration::from_secs(30);

/// Cola de espera de una ruta de candado.
#[derive(Default)]
struct WaiterSlot {
    turn: Arc<tokio::sync::Mutex<()>>,
    blocked: AtomicUsize,
}

/// Una cola por ruta, solo mientras alguien la usa: cada `SlotRef` que se
/// suelta quita la entrada si ya nadie más la tiene (el mapa queda acotado
/// por las esperas en curso, no por las rutas vistas: las de `settings.json`
/// de cada cuenta de `/account/add` también pasan por aquí).
///
/// La clave es la ruta tal como llega, sin canonizar: dos grafías de un mismo
/// archivo (`a/../b`, un enlace) tendrían dos colas y podrían esperar en dos
/// hilos a la vez. No pasa porque todos los llamadores construyen la ruta de
/// la misma forma (`opts.hooks.join(<nombre fijo>)`); el `flock` sigue siendo
/// el mismo archivo, así que la exclusión no se pierde, solo el tope de hilos.
/// No se canoniza a propósito: `canonicalize` toca el disco y falla si el
/// archivo aún no existe.
fn slots() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Arc<WaiterSlot>>> {
    static SLOTS: OnceLock<Mutex<HashMap<PathBuf, Arc<WaiterSlot>>>> = OnceLock::new();
    SLOTS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// Una referencia a la cola de una ruta; al soltar la última (aparte del
/// mapa) la entrada se borra. Todo dueño de un turno tiene una.
struct SlotRef {
    path: PathBuf,
    slot: Arc<WaiterSlot>,
}

impl std::ops::Deref for SlotRef {
    type Target = WaiterSlot;
    fn deref(&self) -> &WaiterSlot {
        &self.slot
    }
}

impl Clone for SlotRef {
    fn clone(&self) -> Self {
        // Bajo el candado del mapa, para que el recuento que mira `drop` sea
        // coherente con las altas de `waiter_slot`.
        let _map = slots();
        SlotRef {
            path: self.path.clone(),
            slot: Arc::clone(&self.slot),
        }
    }
}

impl Drop for SlotRef {
    fn drop(&mut self) {
        let mut map = slots();
        let only_map_and_us = Arc::strong_count(&self.slot) == 2;
        if only_map_and_us
            && map
                .get(&self.path)
                .is_some_and(|kept| Arc::ptr_eq(kept, &self.slot))
        {
            map.remove(&self.path);
        }
    }
}

fn waiter_slot(path: &Path) -> SlotRef {
    let mut map = slots();
    let slot = Arc::clone(map.entry(path.to_path_buf()).or_default());
    SlotRef {
        path: path.to_path_buf(),
        slot,
    }
}

/// ¿Tiene `path` una cola en el mapa ahora? (pruebas).
#[cfg(test)]
fn queued(path: &Path) -> bool {
    slots().contains_key(path)
}

fn timed_out(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!("candado de {} ocupado", path.display()),
    )
}

/// `open(path + ".lock", "a+")` con 0600 si es nuevo (y su directorio).
fn lock_file_for(path: &Path) -> io::Result<fs::File> {
    let mut name = path.as_os_str().to_owned();
    name.push(".lock");
    let lock = std::path::PathBuf::from(name);
    if let Some(dir) = lock.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock)
}

#[cfg(test)]
mod tests {
    use super::{FileLock, queued};
    use std::time::Duration;

    #[tokio::test]
    async fn lock_queues_are_pruned_when_empty() {
        let dir = std::env::temp_dir().join(format!("cmd-files-queue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<_> = (0..40).map(|i| dir.join(format!("c{i}.json"))).collect();
        for path in &paths {
            let lock = FileLock::acquire_timeout(path, Duration::from_secs(5))
                .await
                .unwrap();
            drop(lock);
            assert!(!queued(path), "{} sigue en el mapa", path.display());
            assert_eq!(FileLock::blocked_waiters(path), 0);
            assert!(!queued(path));
        }
        // Con un dueño que no suelta, la espera vence y la cola se vacía igual
        // cuando el hilo que esperaba obtiene el candado.
        let held = FileLock::acquire(&paths[0]).unwrap();
        let late = FileLock::acquire_timeout(&paths[0], Duration::from_millis(50)).await;
        assert!(late.is_err());
        assert!(queued(&paths[0]), "el hilo sigue esperando su turno");
        drop(held);
        for _ in 0..200 {
            if !queued(&paths[0]) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!queued(&paths[0]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
