//! Composición gradual del tablero web portado a Rust.
pub mod assets;
pub mod compose;
pub mod gate;
pub mod registry;
pub mod routes;
pub mod status;

use crate::dash::DashConfig;
use http::Method;
use registry::Resolved;
use std::{path::PathBuf, sync::Mutex};

pub use assets::Manifest;
pub use compose::{ComponentState, Composed, Selection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebRoute {
    Index,
    Gate,
    Ready,
    Status,
    Asset(String),
}

impl WebRoute {
    pub fn route(method: &Method, target: &str, asset_exists: &dyn Fn(&str) -> bool) -> Option<Self> {
        let path = crate::dash::router::path_of(target);
        match (method, path) {
            (&Method::GET, "/") | (&Method::GET, "/index.html") => Some(Self::Index),
            (&Method::GET, "/web/gate.js") => Some(Self::Gate),
            (&Method::POST, "/web/ready") => Some(Self::Ready),
            (&Method::GET, "/web/status") => Some(Self::Status),
            (&Method::GET, p) if p.starts_with("/web/") => {
                let rel = p.trim_start_matches("/web/");
                asset_exists(rel).then(|| Self::Asset(rel.to_string()))
            }
            _ => None,
        }
    }
}

pub struct WebState {
    pub selection_path: PathBuf,
    pub repo_root: Option<PathBuf>,
    pub web_dir: PathBuf,
    pub registry: Resolved,
    pub manifest: Manifest,
    pub selection: Mutex<Selection>,
}

impl WebState {
    pub fn new(cfg: &DashConfig) -> Self {
        let registry = Resolved::from_components_dir(
            cfg.repo_root.clone(),
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/comandos-web/components"),
        );
        let manifest = Manifest::load(&cfg.web_dir).unwrap_or_else(|_| Manifest::default());
        let selection_path = cfg.home.join(".claude/hooks/comandos-web.json");
        Self {
            selection: Mutex::new(Selection::load(&selection_path)),
            selection_path,
            repo_root: cfg.repo_root.clone(),
            web_dir: cfg.web_dir.clone(),
            registry,
            manifest,
        }
    }

    pub fn route_exists(&self) -> impl Fn(&str) -> bool + '_ {
        |rel| assets::valid_relative(rel) && self.web_dir.join(rel).is_file()
    }

    pub fn selection(&self) -> Selection {
        let mut sel = self.selection.lock().unwrap_or_else(|e| e.into_inner());
        sel.refresh(&self.selection_path);
        sel.clone()
    }
}
