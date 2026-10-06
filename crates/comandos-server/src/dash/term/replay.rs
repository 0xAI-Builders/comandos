//! Grabación binaria sin interpretación ni procesos: también se usa para sombra.
pub async fn open(path: &std::path::Path) -> std::io::Result<tokio::fs::File> {
    tokio::fs::File::open(path).await
}
