//! Censo de declinaciones (D7 del plan 2f): qué pide aún el Python y cuántas
//! veces, por `(método, ruta sin consulta)`. Acotado a 512 claves (lo demás
//! cuenta en `"(otras)"`) y escrito de forma atómica: el archivo siempre es
//! JSON válido aunque el proceso muera a mitad de escritura.
use super::files::write_text_atomic;
use http::Method;
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const MAX_KEYS: usize = 512;
const OTHERS: &str = "(otras)";

/// Cada cuánto se escribe el censo mientras el frente corre.
pub const FLUSH_PERIOD: Duration = Duration::from_secs(60);

/// Nombre del archivo en `$XDG_RUNTIME_DIR`. Lleva el puerto: la sombra (4782)
/// no pisa el censo del frente vivo (4777).
pub fn file_name(port: u16) -> String {
    format!("comandos-dash-declines-{port}.json")
}

pub struct DeclineCensus {
    since_ms: i64,
    counts: Mutex<Counts>,
    /// Una escritura a la vez, con la foto tomada DENTRO: la del apagado no
    /// puede quedar pisada por una periódica más vieja que seguía en curso.
    writer: Mutex<()>,
}

#[derive(Default)]
struct Counts {
    by_route: BTreeMap<String, u64>,
    /// Sube con cada declinación.
    generation: u64,
    /// La generación que tiene el archivo (`None`: nunca se escribió).
    written: Option<u64>,
}

impl Default for DeclineCensus {
    fn default() -> Self {
        Self {
            since_ms: super::wall_clock_ms(),
            counts: Mutex::default(),
            writer: Mutex::default(),
        }
    }
}

impl DeclineCensus {
    fn counts(&self) -> std::sync::MutexGuard<'_, Counts> {
        self.counts.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Una declinación de `método ruta?consulta`. La consulta nunca entra en la
    /// clave (lleva `deviceId` y tokens).
    pub fn note(&self, method: &Method, target: &str) {
        let path = target.split_once('?').map_or(target, |(path, _)| path);
        let key = format!("{method} {path}");
        let mut counts = self.counts();
        let full = counts.by_route.len() >= MAX_KEYS && !counts.by_route.contains_key(&key);
        let slot = if full { OTHERS.to_owned() } else { key };
        *counts.by_route.entry(slot).or_insert(0) += 1;
        counts.generation = counts.generation.wrapping_add(1);
    }

    /// La foto y su generación, de una vez bajo el candado.
    fn photo(&self) -> (Value, u64) {
        let counts = self.counts();
        let map: Map<String, Value> = counts
            .by_route
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect();
        (
            json!({"since": self.since_ms, "counts": map}),
            counts.generation,
        )
    }

    /// `{"since": ms, "counts": {"GET /ruta": n, …}}`.
    pub fn snapshot(&self) -> Value {
        self.photo().0
    }

    /// Escribe el censo (temporal + `fsync` + `rename`). Bloquea: desde
    /// `spawn_blocking`.
    pub fn flush(&self, file: &Path) -> io::Result<()> {
        let _writer = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        let (snapshot, generation) = self.photo();
        let text = serde_json::to_string(&snapshot).map_err(io::Error::other)?;
        write_text_atomic(file, &text)?;
        // Lo que llegó durante la escritura queda pendiente para la siguiente.
        self.counts().written = Some(generation);
        Ok(())
    }

    /// Como `flush`, pero solo si hay algo nuevo desde la última escritura (o
    /// nunca se escribió): sin declinaciones nuevas no hay `fsync` cada minuto.
    pub fn flush_if_changed(&self, file: &Path) -> io::Result<()> {
        let pending = {
            let counts = self.counts();
            counts.written != Some(counts.generation)
        };
        if pending {
            self.flush(file)?;
        }
        Ok(())
    }
}

/// Escribe en un hilo de bloqueo; un fallo solo deja una línea en stderr.
pub async fn flush_in_background(census: Arc<DeclineCensus>, file: PathBuf) {
    let shown = file.display().to_string();
    match tokio::task::spawn_blocking(move || census.flush_if_changed(&file)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("comandos dash: censo de declinaciones ({shown}): {error}"),
        Err(error) => eprintln!("comandos dash: censo de declinaciones ({shown}): {error}"),
    }
}

/// La tarea del frente: escribe cada `FLUSH_PERIOD` (el apagado ordenado
/// escribe una última vez aparte). Una sola tarea dormida; ningún hilo.
pub async fn flush_every(census: Arc<DeclineCensus>, file: PathBuf, period: Duration) {
    let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticks.tick().await;
        flush_in_background(census.clone(), file.clone()).await;
    }
}
