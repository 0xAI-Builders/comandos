//! GTK configuration and write authority adapt the shared file policies.
use crate::{
    config::{AppConfig, RunMode},
    guard::{GuardError, WriteGuard},
};
use std::path::Path;
pub type StateError = comandos_desktop::state_files::StateError<GuardError>;
pub type StateFiles = comandos_desktop::state_files::StateFiles<AppConfig, WriteGuard>;
impl comandos_desktop::state_files::StateConfig for AppConfig {
    fn mode(&self) -> RunMode {
        AppConfig::mode(self)
    }
    fn home(&self) -> &Path {
        AppConfig::home(self)
    }
    fn hooks_dir(&self) -> &Path {
        AppConfig::hooks_dir(self)
    }
}
impl comandos_desktop::state_files::StateGuard for WriteGuard {
    type Error = GuardError;
    fn create_dir_all(&self, path: &Path, mode: u32) -> Result<(), GuardError> {
        WriteGuard::create_dir_all(self, path, mode)
    }
    fn open_lock(&self, path: &Path) -> Result<std::fs::File, GuardError> {
        WriteGuard::open_lock(self, path)
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8], prefix: &str) -> Result<(), GuardError> {
        WriteGuard::write_atomic(self, path, bytes, prefix)
    }
    fn write_atomic_when(
        &self,
        path: &Path,
        bytes: &[u8],
        prefix: &str,
        allowed: impl Fn() -> bool,
    ) -> Result<(), GuardError> {
        WriteGuard::write_atomic_when(self, path, bytes, prefix, allowed)
    }
    fn archive_once(&self, path: &Path, bytes: &[u8]) -> Result<bool, GuardError> {
        WriteGuard::archive_once(self, path, bytes)
    }
    fn remove_file(&self, path: &Path) -> Result<(), GuardError> {
        WriteGuard::remove_file(self, path)
    }
}
