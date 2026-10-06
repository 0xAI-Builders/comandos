use crate::guard::{GuardError, WriteGuard};
pub use comandos_desktop::ipc::{IpcError, IpcKind, IpcRequest, read_request, read_request_domain};
pub type IpcConsumer = comandos_desktop::ipc::IpcConsumer<WriteGuard>;
impl comandos_desktop::ipc::IpcGuard for WriteGuard {
    type Error = GuardError;
    fn remove_file(&self, path: &std::path::Path) -> Result<(), GuardError> {
        WriteGuard::remove_file(self, path)
    }
}
