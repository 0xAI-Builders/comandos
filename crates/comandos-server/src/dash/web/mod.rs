//! Composición gradual del tablero web portado a Rust.
pub mod assets;
pub mod compose;
mod file_stamp;
pub mod gate;
pub mod markdown;
pub mod native_page;
pub mod registry;
pub mod routes;
pub mod status;

use crate::dash::DashConfig;
use file_stamp::FileStamp;
use http::Method;
use registry::Resolved;
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

pub use assets::Manifest;
pub use compose::{ComponentState, Composed, Selection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebRoute {
    Index,
    Gate,
    Ready,
    Markdown,
    Status,
    Asset(String),
    NativeAsset(String),
}

impl WebRoute {
    pub fn route(
        method: &Method,
        target: &str,
        asset_exists: &dyn Fn(&str) -> bool,
    ) -> Option<Self> {
        let path = crate::dash::router::path_of(target);
        match (method, path) {
            (&Method::GET, "/") | (&Method::GET, "/index.html") => Some(Self::Index),
            (&Method::GET, "/web/gate.js") => Some(Self::Gate),
            (&Method::POST, "/web/ready") => Some(Self::Ready),
            (&Method::POST, "/web/markdown") => Some(Self::Markdown),
            (&Method::GET, "/web/status") => Some(Self::Status),
            (&Method::GET, p) if p.starts_with("/web/") => {
                let rel = p.trim_start_matches("/web/");
                asset_exists(rel).then(|| Self::Asset(rel.to_string()))
            }
            (&Method::GET, p) if p.starts_with("/assets/") => {
                asset_exists(p).then(|| Self::NativeAsset(p.to_string()))
            }
            _ => None,
        }
    }
}

pub struct WebState {
    pub selection_path: PathBuf,
    home: PathBuf,
    pub repo_root: Option<PathBuf>,
    pub web_dir: PathBuf,
    pub dash_dir: PathBuf,
    pub registry: Resolved,
    pub gate: gate::Gate,
    manifest: Mutex<(Option<FileStamp>, Manifest)>,
    selection: Mutex<(Instant, Option<FileStamp>, Selection)>,
}

impl WebState {
    pub fn new(cfg: &DashConfig) -> Self {
        let registry = Resolved::embedded(cfg.repo_root.clone());
        let manifest = Manifest::load(&cfg.web_dir).unwrap_or_else(|_| Manifest::default());
        let selection_path = cfg.home.join(".claude/hooks/comandos-web.json");
        Self {
            selection: Mutex::new((
                Instant::now(),
                FileStamp::read(&selection_path),
                Selection::load_domain(&cfg.home, &selection_path),
            )),
            selection_path,
            home: cfg.home.clone(),
            repo_root: cfg.repo_root.clone(),
            web_dir: cfg.web_dir.clone(),
            dash_dir: cfg.dash_dir.clone(),
            registry,
            manifest: Mutex::new((
                FileStamp::read(&cfg.web_dir.join(comandos_core::web_assets::MANIFEST_FILE)),
                manifest,
            )),
            gate: gate::Gate::default(),
        }
    }

    pub fn route_exists(&self) -> impl Fn(&str) -> bool + '_ {
        |rel| {
            (assets::valid_relative(rel) && self.web_dir.join(rel).is_file())
                || native_page::alias(&self.manifest(), &self.web_dir, &self.dash_dir, rel)
                    .is_some()
        }
    }

    pub fn selection(&self) -> Selection {
        let mut sel = self.selection.lock().unwrap_or_else(|e| e.into_inner());
        if sel.0.elapsed() >= Duration::from_secs(1) {
            // SQL authority can change without any legacy file mtime event.
            sel.2 = Selection::load_domain(&self.home, &self.selection_path);
            sel.1 = FileStamp::read(&self.selection_path);
            sel.0 = Instant::now();
        }
        sel.2.clone()
    }

    pub fn manifest(&self) -> Manifest {
        let mut cache = self.manifest.lock().unwrap_or_else(|e| e.into_inner());
        let stamp = FileStamp::read(&self.web_dir.join(comandos_core::web_assets::MANIFEST_FILE));
        if stamp != cache.0 || cache.1.check_files(&self.web_dir).is_err() {
            cache.1 = Manifest::load(&self.web_dir).unwrap_or_default();
            cache.0 = stamp;
        }
        // A manifest remaining unchanged cannot certify that artifacts are still
        // present. Fail closed on deletion and recover when the build is restored.
        if cache.1.check_files(&self.web_dir).is_err() {
            return Manifest::default();
        }
        cache.1.clone()
    }
}
