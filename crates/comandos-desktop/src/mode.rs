#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// tmux propio (`-L`), hooks temporales: desarrollo y pruebas.
    Sandbox,
    /// tmux del usuario con clientes de solo lectura; ninguna escritura.
    Shadow,
    /// Sustituto real de `cc-app`.
    Live,
}
