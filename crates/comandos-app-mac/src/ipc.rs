//! macOS file IPC: seed before polling, retain producer-owned files, finite paths.
#![forbid(unsafe_code)]
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Focus,
    Open,
    Close,
}
pub struct Ipc {
    root: PathBuf,
    home: Option<PathBuf>,
    mtimes: [f64; 3],
}
const PATHS: [(Kind, &str); 3] = [
    (Kind::Focus, "app-focus.json"),
    (Kind::Open, "app-tab-open.json"),
    (Kind::Close, "app-tab-close.json"),
];
fn mtime(m: &fs::Metadata) -> f64 {
    m.mtime() as f64 + m.mtime_nsec() as f64 / 1e9
}
impl Ipc {
    pub fn new(root: PathBuf) -> Self {
        let mtimes =
            PATHS.map(|(_, name)| fs::symlink_metadata(root.join(name)).map_or(0., |m| mtime(&m)));
        Self {
            root,
            mtimes,
            home: None,
        }
    }
    pub fn new_domain(home: PathBuf, root: PathBuf) -> Self {
        let mut ipc = Self::new(root);
        ipc.home = Some(home);
        ipc
    }
    pub fn poll(&mut self) -> Vec<(Kind, Value)> {
        let mut out = vec![];
        for (i, (kind, name)) in PATHS.iter().enumerate() {
            let path = self.root.join(name);
            if let Some(home) = &self.home {
                let Ok((mode, row)) = comandos_store::domains::commands::peek(home, name) else {
                    continue;
                };
                if matches!(
                    mode,
                    comandos_store::unified::Mode::Unified | comandos_store::unified::Mode::Sealed
                ) {
                    if let Some((seq, body)) = row
                        && body.len() <= 1 << 20
                        && let Some(value) = comandos_core::json::workspace_loads_bytes(&body)
                    {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
                        if comandos_store::domains::commands::acknowledge(
                            home,
                            name,
                            Some(seq),
                            &body,
                            now,
                            || Ok(()),
                        )
                        .is_ok()
                        {
                            out.push((*kind, value));
                        }
                    }
                    continue;
                }
            }
            let Ok(before) = fs::symlink_metadata(&path) else {
                continue;
            };
            let stamp = mtime(&before);
            if stamp == 0. || stamp == self.mtimes[i] {
                continue;
            }
            self.mtimes[i] = stamp;
            if !before.is_file() || before.len() > 1 << 20 {
                continue;
            }
            let Ok(file) = OpenOptions::new()
                .read(true)
                .custom_flags(
                    (nix::fcntl::OFlag::O_NONBLOCK
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_NOCTTY)
                        .bits(),
                )
                .open(&path)
            else {
                continue;
            };
            let Ok(pinned) = file.metadata() else {
                continue;
            };
            if !pinned.is_file()
                || pinned.dev() != before.dev()
                || pinned.ino() != before.ino()
                || mtime(&pinned) != stamp
                || pinned.len() > 1 << 20
            {
                continue;
            }
            let mut text = String::new();
            if file.take((1 << 20) + 1).read_to_string(&mut text).is_err() || text.len() > 1 << 20 {
                continue;
            }
            if !fs::symlink_metadata(&path).is_ok_and(|after| {
                after.is_file()
                    && after.dev() == pinned.dev()
                    && after.ino() == pinned.ino()
                    && mtime(&after) == stamp
                    && after.len() == pinned.len()
            }) {
                continue;
            }
            if let Ok(value) = comandos_core::json::workspace_loads(&text) {
                if let Some(home) = &self.home {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
                    let _ = comandos_store::domains::commands::acknowledge(
                        home,
                        name,
                        None,
                        text.as_bytes(),
                        now,
                        || Ok(()),
                    );
                }
                out.push((*kind, value));
            }
        }
        out
    }
}
