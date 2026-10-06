# Oráculos compartidos congelados — alcance verificado

El helper registra, verifica deriva y reproduce dorados. Replay es el valor por omisión y nunca ejecuta la clausura del oráculo; un caso ausente falla explícitamente. Los efectos sobre archivos, enlaces, permisos, directorios y SQLite se guardan como artefactos lógicos. Replay aplica solamente los cambios del oráculo y conserva los SQLite nativos abiertos que no cambió.

Los 21 targets de abajo pasan record, check y replay: 117 pruebas runtime y 19 store. El fallo final de claves foráneas se reprodujo en una prueba del helper y se corrigió; el target news volvió a pasar los tres modos. Replay se ejecutó con un python3 falso que termina en 127; el canario de llamadas no se creó. Las bases SQLite gemelas bajo el mismo fixture privado se reproducen y siguen comparándose fila por fila con el resultado nativo. Los IDs Grok derivados de rutas se normalizan usando SHA-256 de los componentes especificados por la referencia; replay los rehidrata para la ruta real del fixture actual.

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

Los siete callers adicionales también quedan congelados:

- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/extension_inventory_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/extension_prepare_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/pane_extensions_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/session_configuration_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/session_profiles_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-runtime/tests/tmux_snapshot_oracle.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-oracle-freeze-finish/crates/comandos-store/tests/news_oracle.rs`

Las comprobaciones nativas de PID, tmux y archivos siguen ejecutándose sobre el fixture actual. Las identidades de procesos se leen del tmux privado y de sus procesos; se rehidratan los pares PID/start sin reemplazar los resultados nativos. Los bundles de prepare conservan contenido de referencia y SHA independientes; replay vuelve a calcular el SHA esperado desde esos bytes. Los IDs de rutas y los tiempos de caché se rehidratan desde el fixture actual. SQLite se identifica por esquema canónico y valores, no por distribución de páginas, y se restaura completo antes de aplicar claves foráneas.

Se verificaron seis pruebas del helper, Clippy de todos los targets de oracle/runtime/store/xtask y la ejecución de test-map. El alcance de este incremento son estos 21 targets; no declara replay sin Python del workspace completo.

Siguen fuera de este incremento los wrappers propios de servidor y launch-command, las llamadas directas de core/store/app y el cambio de xtask parity a respuestas grabadas. No se retiraron originales ni se ejecutó un corte en vivo.
