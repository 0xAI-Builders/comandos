use crate::{
    config::BrokerConfig,
    pool::Pool,
    session::{Catalog, Registry, ToolBackend, handle_client},
    wire::encode_status,
};
use serde_json::Value;
use std::{future::Future, io, path::Path, path::PathBuf, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::watch, task::JoinSet};

pub struct Broker;

impl Broker {
    pub async fn serve(cfg: BrokerConfig, shutdown: impl Future<Output = ()>) -> io::Result<()> {
        std::fs::create_dir_all(&cfg.state_dir)?;
        let listener = TcpListener::bind(("127.0.0.1", cfg.port)).await?;
        let catalog_value = cfg.catalog.clone();
        let registry = Arc::new(Registry::new());
        let pool = Pool::new(cfg, registry.clone());
        let catalog = Arc::new(Catalog::from_value(&catalog_value));
        let backend: Arc<dyn ToolBackend> = Arc::new(pool.clone());
        let (tx, rx) = watch::channel(false);
        let house = tokio::spawn(housekeeping(pool.clone(), pool.state_dir(), rx));
        let mut clients = JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                Some(_) = clients.join_next() => {}
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    clients.spawn(handle_client(stream, catalog.clone(), backend.clone(), registry.clone()));
                }
            }
        }
        registry.close();
        clients.abort_all();
        while clients.join_next().await.is_some() {}
        pool.close_all().await;
        let _ = tx.send(true);
        let _ = house.await;
        Ok(())
    }
}

pub async fn housekeeping(pool: Arc<Pool>, state_dir: PathBuf, mut stop: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = stop.changed() => break,
        }
        pool.reap_idle().await;
        pool.retry_stuck().await;
        if let Err(e) = write_status(&state_dir, &pool.status().await) {
            eprintln!("comandos-broker-mac: no se pudo escribir status.json: {e}");
        }
    }
}

pub fn write_status(dir: &Path, status: &Value) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = encode_status(status)?;
    let tmp = dir.join("status.json.tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join("status.json")).map_err(|e| e.to_string())
}
