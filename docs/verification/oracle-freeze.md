# Oráculos compartidos congelados — alcance verificado

El helper registra, verifica deriva y reproduce dorados. Replay es el valor por omisión y nunca ejecuta la clausura del oráculo; un caso ausente falla explícitamente. Los efectos sobre archivos, enlaces, permisos, directorios y SQLite se guardan como artefactos lógicos. Replay aplica solamente los cambios del oráculo y conserva los SQLite nativos abiertos que no cambió.

Los 14 targets de abajo pasan record, check y replay: 80 pruebas runtime y 14 store. Replay se ejecutó con un python3 falso que termina en 127; el canario de llamadas no se creó. Las bases SQLite gemelas bajo el mismo fixture privado se reproducen y siguen comparándose fila por fila con el resultado nativo. Los IDs Grok derivados de rutas se normalizan usando SHA-256 de los componentes especificados por la referencia; replay los rehidrata para la ruta real del fixture actual.

Targets verificados:

- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/cli_catalog_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/extension_observations_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/limits_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/model_watch_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/news_editions_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/pane_snapshot_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/providers_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/providers_public_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/ssh_config_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/tui_state_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-store/tests/session_profiles_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-store/tests/usage_change_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-store/tests/usage_import_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-store/tests/usage_read_oracle.rs`

Límites pendientes concretos: estos siete callers aún conservan su ruta de intérprete original. No se ha declarado verde un pase sin Python de esos targets ni del workspace completo. Requieren tratar clock/UUID, IDs de rutas o datos del laboratorio tmux sin retirar sus aserciones nativas:

- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/extension_inventory_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/extension_prepare_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/pane_extensions_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/session_configuration_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/session_profiles_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/tmux_snapshot_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-store/tests/news_oracle.rs`

También siguen pendientes los wrappers propios de servidor y launch-command, las llamadas directas de core/store/app y el cambio de xtask parity a respuestas grabadas. No se retiraron originales ni se ejecutó un corte en vivo.
