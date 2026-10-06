# Fixture T12 mixta

El exportador de Rust crea estado nuevo, sin iniciar tmux ni agentes. Se compiló y ejecutó desde `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/examples/export_gtk_fixture.rs`. Rechaza una raíz existente y escribe los JSON mediante WriteGuard.

La muestra comprobada está en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/.migration-build/t12-mixed/run/comandos-app-sbx`. Contiene snapshots con checksum válido: dos panes con identidades Claude/Codex sintéticas y una sesión shell; además, local, SSH sintético, xterm tipado y sesión ausente. La distribución GTK tiene splits anidados x/y. El documento de tablero incluye Bruno, varias filas, favorito, dots y marcas.

El arranque sandbox sin tablero consume el documento de fixture dentro de su raíz privada y usa SandboxScopes: las identidades guardadas nunca ejecutan los CLI de los agentes. Docking/orden usan el documento privado y las marcas cambian solo el estado visual privado. Cerrar un split requiere un backend explícito; no hay un backend destructor simulado oculto en el exportador.

El controlador remoto debe regenerar la muestra con el ejecutable export_gtk_fixture y una ruta absoluta nueva en ese equipo: los cwd quedan asociados a la HOME privada de esa muestra. Copiar el JSON local sin cambiar los cwd no produce un fixture remoto válido. La ejecución de GTK, los gestos y las capturas remotas siguen pendientes; esta muestra no acredita el gate visual T12.
